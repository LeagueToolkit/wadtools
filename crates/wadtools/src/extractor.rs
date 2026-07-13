use crate::utils::{is_hex_chunk_path, truncate_middle, WadHashtable};
use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, Ok};
use dashmap::DashMap;
use eyre::Context;
use fancy_regex::Regex;
use league_toolkit::{
    file::LeagueFileKind,
    wad::{decompress_raw, Wad, WadChunk},
};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

const MAX_LOG_PATH_LEN: usize = 120;

pub struct ExtractStats {
    pub extracted_count: usize,
    pub skipped_existing: usize,
    pub bytes_written: u64,
    pub by_type: HashMap<LeagueFileKind, usize>,
}

enum ChunkResult {
    Extracted(LeagueFileKind, u64),
    SkippedFilter,
    SkippedExisting,
}

pub struct Extractor<'a> {
    wad: &'a mut Wad<File>,
    hashtable: &'a WadHashtable,
    /// Names recovered from `.bin` files for this WAD, consulted before the shared
    /// hashtable. Gap-fill only — never contains a hash the hashtable already resolves.
    discovered: HashMap<u64, Arc<str>>,
    filter_pattern: Option<Regex>,
    hash_filter: Option<Vec<u64>>,
    filter_invert: bool,
}

impl<'a> Extractor<'a> {
    pub fn new(wad: &'a mut Wad<File>, hashtable: &'a WadHashtable) -> Self {
        Self {
            wad,
            hashtable,
            discovered: HashMap::new(),
            filter_pattern: None,
            hash_filter: None,
            filter_invert: false,
        }
    }

    pub fn set_discovered(&mut self, discovered: HashMap<u64, Arc<str>>) {
        self.discovered = discovered;
    }

    /// Resolves a chunk's path, preferring names recovered from bins over the shared
    /// hashtable (which falls back to a hex string when the hash is unknown).
    fn resolve_path(&self, path_hash: u64) -> Arc<str> {
        self.discovered
            .get(&path_hash)
            .cloned()
            .unwrap_or_else(|| self.hashtable.resolve_path(path_hash))
    }

    pub fn set_filter_pattern(&mut self, filter_pattern: Option<Regex>) {
        self.filter_pattern = filter_pattern;
    }

    pub fn set_hash_filter(&mut self, hash_filter: Option<Vec<u64>>) {
        self.hash_filter = hash_filter;
    }

    pub fn set_filter_invert(&mut self, filter_invert: bool) {
        self.filter_invert = filter_invert;
    }

    pub fn extract_chunks(
        &mut self,
        extract_directory: impl AsRef<Utf8Path>,
        filter_type: Option<&[LeagueFileKind]>,
        overwrite: bool,
    ) -> eyre::Result<ExtractStats> {
        let extract_directory = extract_directory.as_ref().to_path_buf();

        let chunks: Vec<WadChunk> = self.wad.chunks().iter().copied().collect();
        let total = chunks.len() as u64;

        let span = tracing::info_span!("extract", total = total);
        let _entered = span.enter();
        span.pb_set_style(
            &ProgressStyle::with_template("{wide_bar:40.cyan/blue} {pos}/{len} \n {spinner} {msg}")
                .unwrap(),
        );
        span.pb_set_length(total);
        span.pb_set_message("Extracting chunks");

        // Bounded channel: caps in-flight raw chunks to limit memory usage.
        // The sequential reader blocks when the channel is full (workers are busy).
        let buffer_size = rayon::current_num_threads().max(1);
        let (tx, rx) = mpsc::sync_channel::<(WadChunk, String, Box<[u8]>)>(buffer_size);

        let counter = AtomicUsize::new(0);
        let extracted_counter = AtomicUsize::new(0);
        let skipped_existing_counter = AtomicUsize::new(0);
        let bytes_written_counter = AtomicU64::new(0);
        let by_type: DashMap<LeagueFileKind, usize> = DashMap::new();
        let filter_invert = self.filter_invert;
        let extract_dir = &extract_directory;
        let err_holder: std::sync::Mutex<Option<eyre::Report>> = std::sync::Mutex::new(None);

        std::thread::scope(|s| -> eyre::Result<()> {
            // Worker thread: receive chunks and decompress+write in parallel via rayon::scope
            let worker = s.spawn(|| {
                rayon::scope(|rayon_scope| {
                    for (chunk, path_str, raw) in rx {
                        let counter = &counter;
                        let extracted_counter = &extracted_counter;
                        let skipped_existing_counter = &skipped_existing_counter;
                        let bytes_written_counter = &bytes_written_counter;
                        let by_type = &by_type;
                        let err_holder = &err_holder;
                        let progress_span = &span;

                        rayon_scope.spawn(move |_| {
                            let result = process_chunk(
                                &chunk,
                                &path_str,
                                &raw,
                                extract_dir,
                                filter_type,
                                filter_invert,
                                overwrite,
                            );

                            match result {
                                std::result::Result::Ok(ChunkResult::Extracted(kind, size)) => {
                                    extracted_counter.fetch_add(1, Ordering::Relaxed);
                                    bytes_written_counter.fetch_add(size, Ordering::Relaxed);
                                    *by_type.entry(kind).or_insert(0) += 1;
                                }
                                std::result::Result::Ok(ChunkResult::SkippedExisting) => {
                                    skipped_existing_counter.fetch_add(1, Ordering::Relaxed);
                                }
                                std::result::Result::Ok(ChunkResult::SkippedFilter) => {}
                                Err(e) => {
                                    let mut guard = err_holder.lock().unwrap();
                                    if guard.is_none() {
                                        *guard = Some(e);
                                    }
                                }
                            }

                            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                            progress_span.pb_set_position(done as u64);
                        });
                    }
                });
                // rayon::scope blocks until all spawned tasks complete
            });

            // Sequential reader: read raw bytes and send through bounded channel
            for chunk in chunks.iter() {
                if err_holder.lock().unwrap().is_some() {
                    break;
                }

                // Hash filter is the cheapest check — do it before resolving path
                if should_skip_hash(
                    chunk.path_hash,
                    self.hash_filter.as_deref(),
                    self.filter_invert,
                ) {
                    let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    span.pb_set_position(done as u64);
                    continue;
                }

                let chunk_path_str = self.resolve_path(chunk.path_hash);

                span.pb_set_message(&truncate_middle(chunk_path_str.as_ref(), MAX_LOG_PATH_LEN));

                if should_skip_pattern(
                    chunk_path_str.as_ref(),
                    self.filter_pattern.as_ref(),
                    self.filter_invert,
                ) {
                    let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    span.pb_set_position(done as u64);
                    continue;
                }

                let raw = self.wad.load_chunk_raw(chunk).wrap_err(format!(
                    "failed to read raw chunk (chunk_path: {})",
                    chunk_path_str.as_ref()
                ))?;

                // Blocks if the channel is full, bounding memory
                let _ = tx.send((*chunk, chunk_path_str.to_string(), raw));
            }

            drop(tx);
            worker.join().unwrap();

            Ok(())
        })?;

        if let Some(err) = err_holder.into_inner().unwrap() {
            return Err(err);
        }

        let by_type_map: HashMap<LeagueFileKind, usize> = by_type.into_iter().collect();

        Ok(ExtractStats {
            extracted_count: extracted_counter.load(Ordering::Relaxed),
            skipped_existing: skipped_existing_counter.load(Ordering::Relaxed),
            bytes_written: bytes_written_counter.load(Ordering::Relaxed),
            by_type: by_type_map,
        })
    }
}

fn process_chunk(
    chunk: &WadChunk,
    path_str: &str,
    raw: &[u8],
    extract_dir: &Utf8Path,
    filter_type: Option<&[LeagueFileKind]>,
    filter_invert: bool,
    overwrite: bool,
) -> eyre::Result<ChunkResult> {
    let chunk_data = decompress_raw(raw, chunk.compression_type, chunk.uncompressed_size)
        .wrap_err(format!(
            "failed to decompress chunk (chunk_path: {})",
            path_str
        ))?;

    let chunk_kind = LeagueFileKind::identify_from_bytes(&chunk_data);
    if should_skip_type(chunk_kind, filter_type, filter_invert) {
        return Ok(ChunkResult::SkippedFilter);
    }

    let chunk_path = Utf8Path::new(path_str);
    let final_path = resolve_final_chunk_path(extract_dir, chunk_path, &chunk_data, chunk_kind);
    let full_path = extract_dir.join(&final_path);

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }

    let size = chunk_data.len() as u64;
    match write_chunk_file(full_path.as_std_path(), &chunk_data, overwrite) {
        std::result::Result::Ok(ChunkWriteResult::Written) => {
            return Ok(ChunkResult::Extracted(chunk_kind, size));
        }
        std::result::Result::Ok(ChunkWriteResult::SkippedExisting) => {
            return Ok(ChunkResult::SkippedExisting);
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidFilename => {
            return write_long_filename_chunk(
                chunk,
                final_path,
                extract_dir,
                &chunk_data,
                chunk_kind,
                overwrite,
            );
        }
        Err(error) => {
            return Err(error).wrap_err(format!(
                "failed to write chunk (chunk_path: {})",
                truncate_middle(full_path.as_str(), MAX_LOG_PATH_LEN)
            ));
        }
    }
}

enum ChunkWriteResult {
    Written,
    SkippedExisting,
}

/// Writes chunk data to a file. When `overwrite` is false, uses `create_new(true)` for an
/// atomic existence check, returning `SkippedExisting` on `AlreadyExists`. This avoids the
/// TOCTOU race of a separate exists() check followed by write().
fn write_chunk_file(
    path: &std::path::Path,
    data: &[u8],
    overwrite: bool,
) -> io::Result<ChunkWriteResult> {
    if overwrite {
        fs::write(path, data)?;
        return std::result::Result::Ok(ChunkWriteResult::Written);
    }

    match OpenOptions::new().write(true).create_new(true).open(path) {
        std::result::Result::Ok(mut file) => {
            file.write_all(data)?;
            std::result::Result::Ok(ChunkWriteResult::Written)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            tracing::debug!("skipping existing file: {}", path.display());
            std::result::Result::Ok(ChunkWriteResult::SkippedExisting)
        }
        Err(e) => Err(e),
    }
}

fn resolve_final_chunk_path(
    extract_directory: impl AsRef<Utf8Path>,
    chunk_path: impl AsRef<Utf8Path>,
    chunk_data: &[u8],
    chunk_kind: LeagueFileKind,
) -> Utf8PathBuf {
    let mut final_path = chunk_path.as_ref().to_path_buf();

    // Hashed filenames should keep the 16-hex base, but we can append a real extension
    if is_hex_chunk_path(final_path.as_path()) {
        if let Some(ext) = chunk_kind.extension() {
            final_path.set_extension(ext);
        }
        return final_path;
    }

    // - If the original path has no extension, affix .ltk (and real extension if known)
    // - OR if the destination path collides with an existing directory, affix .ltk
    let has_extension = final_path.extension().is_some();
    let collides_with_dir = extract_directory.as_ref().join(&final_path).is_dir();
    if !has_extension || collides_with_dir {
        let original_stem = chunk_path.as_ref().file_stem().unwrap_or("");
        let new_name = build_ltk_name(original_stem, chunk_data);
        final_path.set_file_name(&new_name);
    }

    final_path
}

fn build_ltk_name(file_stem: &str, chunk_data: &[u8]) -> String {
    let kind = LeagueFileKind::identify_from_bytes(chunk_data);
    match kind.extension() {
        Some(ext) => format!("{}.ltk.{}", file_stem, ext),
        None => format!("{}.ltk", file_stem),
    }
}

/// Returns true if the chunk should be skipped based on the pattern filter.
pub(crate) fn should_skip_pattern(
    path: &str,
    filter_pattern: Option<&Regex>,
    filter_invert: bool,
) -> bool {
    if let Some(regex) = filter_pattern {
        let matched = regex.is_match(path).unwrap_or(false);
        return matched == filter_invert;
    }
    false
}

/// Returns true if the chunk should be skipped based on the hash filter.
pub(crate) fn should_skip_hash(
    path_hash: u64,
    hash_filter: Option<&[u64]>,
    filter_invert: bool,
) -> bool {
    hash_filter.is_some_and(|hashes| hashes.contains(&path_hash) == filter_invert)
}

/// Returns true if the chunk should be skipped based on the type filter.
pub(crate) fn should_skip_type(
    chunk_kind: LeagueFileKind,
    filter_type: Option<&[LeagueFileKind]>,
    filter_invert: bool,
) -> bool {
    filter_type.is_some_and(|filter| filter.contains(&chunk_kind) == filter_invert)
}

fn write_long_filename_chunk(
    chunk: &WadChunk,
    chunk_path: impl AsRef<Utf8Path>,
    extract_directory: impl AsRef<Utf8Path>,
    chunk_data: &[u8],
    chunk_kind: LeagueFileKind,
    overwrite: bool,
) -> eyre::Result<ChunkResult> {
    let mut hashed_path = Utf8PathBuf::from(format!("{:016x}", chunk.path_hash));
    if let Some(ext) = chunk_kind.extension() {
        hashed_path.set_extension(ext);
    }

    let full_path = extract_directory.as_ref().join(&hashed_path);

    let disp = chunk_path.as_ref().as_str().to_string();
    let truncated = truncate_middle(&disp, MAX_LOG_PATH_LEN);
    tracing::warn!(
        "Long filename detected (chunk_path: {}, hashed_path: {})",
        truncated,
        &hashed_path
    );

    let size = chunk_data.len() as u64;
    match write_chunk_file(full_path.as_std_path(), chunk_data, overwrite)? {
        ChunkWriteResult::Written => Ok(ChunkResult::Extracted(chunk_kind, size)),
        ChunkWriteResult::SkippedExisting => Ok(ChunkResult::SkippedExisting),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> Regex {
        Regex::new(pattern).unwrap()
    }

    // --- should_skip_pattern tests ---

    #[test]
    fn no_pattern_never_skips() {
        assert!(!should_skip_pattern("anything.dds", None, false));
        assert!(!should_skip_pattern("anything.dds", None, true));
    }

    #[test]
    fn pattern_includes_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(!should_skip_pattern("textures/foo.dds", Some(&re), false));
    }

    #[test]
    fn pattern_excludes_non_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(should_skip_pattern("sounds/bar.wav", Some(&re), false));
    }

    #[test]
    fn pattern_inverted_excludes_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(should_skip_pattern("textures/foo.dds", Some(&re), true));
    }

    #[test]
    fn pattern_inverted_includes_non_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(!should_skip_pattern("sounds/bar.wav", Some(&re), true));
    }

    // --- should_skip_type tests ---

    #[test]
    fn no_type_filter_never_skips() {
        assert!(!should_skip_type(LeagueFileKind::Png, None, false));
        assert!(!should_skip_type(LeagueFileKind::Png, None, true));
    }

    #[test]
    fn type_filter_includes_matching() {
        let types = [LeagueFileKind::Png];
        assert!(!should_skip_type(LeagueFileKind::Png, Some(&types), false));
    }

    #[test]
    fn type_filter_excludes_non_matching() {
        let types = [LeagueFileKind::Png];
        assert!(should_skip_type(LeagueFileKind::Jpeg, Some(&types), false));
    }

    #[test]
    fn type_filter_inverted_excludes_matching() {
        let types = [LeagueFileKind::Png];
        assert!(should_skip_type(LeagueFileKind::Png, Some(&types), true));
    }

    #[test]
    fn type_filter_inverted_includes_non_matching() {
        let types = [LeagueFileKind::Png];
        assert!(!should_skip_type(LeagueFileKind::Jpeg, Some(&types), true));
    }
}
