use crate::utils::{is_hex_chunk_path, truncate_middle, WadHashtable};
use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::eyre::{self, Ok};
use eyre::Context;
use fancy_regex::Regex;
use league_toolkit::{
    file::LeagueFileKind,
    wad::{WadChunk, WadDecoder},
};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Seek},
};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

const MAX_LOG_PATH_LEN: usize = 120;

pub struct Extractor<'chunks> {
    decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
    hashtable: &'chunks WadHashtable,
    filter_pattern: Option<Regex>,
}

impl<'chunks> Extractor<'chunks> {
    pub fn new(
        decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
        hashtable: &'chunks WadHashtable,
    ) -> Self {
        Self {
            decoder,
            hashtable,
            filter_pattern: None,
        }
    }

    pub fn set_filter_pattern(&mut self, filter_pattern: Option<Regex>) {
        self.filter_pattern = filter_pattern;
    }

    pub fn extract_chunks(
        &mut self,
        chunks: &HashMap<u64, WadChunk>,
        extract_directory: impl AsRef<Utf8Path>,
        filter_type: Option<&[LeagueFileKind]>,
    ) -> eyre::Result<usize> {
        let total = chunks.len() as u64;
        let span = tracing::info_span!("extract", total = total);
        let _entered = span.enter();
        span.pb_set_style(
            &ProgressStyle::with_template("{wide_bar:40.cyan/blue} {pos}/{len} \n {spinner} {msg}")
                .unwrap(),
        );
        span.pb_set_length(total);
        span.pb_set_message("Extracting chunks");
        span.pb_set_finish_message("Extraction complete");

        extract_wad_chunks(
            self.decoder,
            chunks,
            self.hashtable,
            extract_directory.as_ref().to_path_buf(),
            |progress, message| {
                // progress is 0.0..1.0; convert to absolute position
                let position = (progress * total as f64).round() as u64;
                span.pb_set_position(position);
                if let Some(msg) = message {
                    span.pb_set_message(msg);
                }
                Ok(())
            },
            filter_type,
            self.filter_pattern.as_ref(),
        )
    }
}

pub fn extract_wad_chunks<TSource: Read + Seek>(
    decoder: &mut WadDecoder<TSource>,
    chunks: &HashMap<u64, WadChunk>,
    wad_hashtable: &WadHashtable,
    extract_directory: Utf8PathBuf,
    report_progress: impl Fn(f64, Option<&str>) -> eyre::Result<()>,
    filter_type: Option<&[LeagueFileKind]>,
    filter_pattern: Option<&Regex>,
) -> eyre::Result<usize> {
    let mut i = 0;
    let mut extracted_count = 0;
    for chunk in chunks.values() {
        let chunk_path_str = wad_hashtable.resolve_path(chunk.path_hash());
        let chunk_path = Utf8Path::new(chunk_path_str.as_ref());

        // advance progress for every chunk (including ones we skip)
        let truncated = truncate_middle(chunk_path_str.as_ref(), MAX_LOG_PATH_LEN);
        report_progress(i as f64 / chunks.len() as f64, Some(truncated.as_str()))?;

        if let Some(regex) = filter_pattern {
            if !regex.is_match(chunk_path_str.as_ref()).unwrap_or(false) {
                i += 1;
                continue;
            }
        }

        if extract_wad_chunk(decoder, chunk, chunk_path, &extract_directory, filter_type)? {
            extracted_count += 1;
        }

        i += 1;
    }

    Ok(extracted_count)
}

pub fn extract_wad_chunk<'wad, TSource: Read + Seek>(
    decoder: &mut WadDecoder<'wad, TSource>,
    chunk: &WadChunk,
    chunk_path: impl AsRef<Utf8Path>,
    extract_directory: impl AsRef<Utf8Path>,
    filter_type: Option<&[LeagueFileKind]>,
) -> eyre::Result<bool> {
    let chunk_data = decoder.load_chunk_decompressed(chunk).wrap_err(format!(
        "failed to decompress chunk (chunk_path: {})",
        chunk_path.as_ref().as_str()
    ))?;

    let chunk_kind = LeagueFileKind::identify_from_bytes(&chunk_data);
    if filter_type.is_some_and(|filter| !filter.contains(&chunk_kind)) {
        tracing::debug!(
            "skipping chunk (chunk_path: {}, chunk_kind: {:?})",
            chunk_path.as_ref().as_str(),
            chunk_kind
        );
        return Ok(false);
    }

    let chunk_path =
        resolve_final_chunk_path(&extract_directory, chunk_path, &chunk_data, chunk_kind);
    let full_path = extract_directory.as_ref().join(&chunk_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent.as_std_path())?;
    }
    let Err(error) = fs::write(full_path.as_std_path(), &chunk_data) else {
        return Ok(true);
    };

    // This will happen if the filename is too long
    if error.kind() == io::ErrorKind::InvalidFilename {
        write_long_filename_chunk(
            chunk,
            chunk_path,
            extract_directory,
            &chunk_data,
            chunk_kind,
        )?;
        Ok(true)
    } else {
        Err(error).wrap_err(format!(
            "failed to write chunk (chunk_path: {})",
            truncate_middle(full_path.as_str(), MAX_LOG_PATH_LEN)
        ))
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

fn write_long_filename_chunk(
    chunk: &WadChunk,
    chunk_path: impl AsRef<Utf8Path>,
    extract_directory: impl AsRef<Utf8Path>,
    chunk_data: &[u8],
    chunk_kind: LeagueFileKind,
) -> eyre::Result<()> {
    let mut hashed_path = Utf8PathBuf::from(format!("{:016x}", chunk.path_hash()));
    if let Some(ext) = chunk_kind.extension() {
        hashed_path.set_extension(ext);
    }

    let disp = chunk_path.as_ref().as_str().to_string();
    let truncated = truncate_middle(&disp, MAX_LOG_PATH_LEN);
    tracing::warn!(
        "Long filename detected (chunk_path: {}, hashed_path: {})",
        truncated,
        &hashed_path
    );

    fs::write(
        extract_directory.as_ref().join(hashed_path).as_std_path(),
        chunk_data,
    )?;

    Ok(())
}
