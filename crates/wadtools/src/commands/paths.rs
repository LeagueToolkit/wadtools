//! Rip the resolvable chunk paths of one or more WADs into a shareable hashtable:
//! either the CDragon hashtable format (a `<hex-hash> <path>` text file) or a mimir
//! `.lhdb` hash table.
//!
//! "Resolvable" means every chunk whose hash we can attribute to a real path -
//! names already known to the shared mimir cache *plus* names recovered by
//! scanning the WAD's `.bin` files (dependency links and string properties, see
//! [`crate::bin_scan`]). Chunks that would only render as their 16-hex fallback are
//! skipped, so the output is a clean, human-meaningful path list.
//!
//! The `.lhdb` output is written in the Game-table configuration (64-bit XXH64 keys,
//! case-insensitive), so it is a drop-in supplemental table for any LeagueToolkit
//! tool that reads the mimir format.

use camino::Utf8Path;
use color_eyre::owo_colors::OwoColorize;
use eyre::Context;
use league_toolkit::wad::Wad;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;

use ltk_hashdb::{Casing, Compression, HashDbWriter, HashKind, KeyWidth};

use crate::{
    bin_scan::scan_wad_bin_paths,
    extractor::should_skip_pattern,
    utils::{create_filter_pattern, format_chunk_path_hash, WadHashtable},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum PathsFormat {
    /// CDragon hashtable format: `<hex-hash> <path>` text file, one entry per line
    #[default]
    Txt,
    /// Mimir hashtable format: `.lhdb` table (Game-format: 64-bit XXH64 keys, case-insensitive)
    Lhdb,
}

impl PathsFormat {
    fn extension(self) -> &'static str {
        match self {
            PathsFormat::Txt => "txt",
            PathsFormat::Lhdb => "lhdb",
        }
    }
}

pub struct PathsArgs {
    /// Input WAD paths, already resolved and directory-expanded.
    pub inputs: Vec<String>,
    /// Explicit output file. When `None`, a sensible sibling path is derived.
    pub output: Option<String>,
    /// Output format. When `None`, inferred from the output extension (defaulting to txt).
    pub format: Option<PathsFormat>,
    pub pattern: Option<String>,
    pub filter_invert: bool,
    /// Scan `.bin` files to recover otherwise-anonymous chunk names (default: true).
    pub resolve_bin_paths: bool,
    /// Decompress every chunk to magic-detect bins, recovering the most names.
    pub full_bin_scan: bool,
    pub show_stats: bool,
}

/// Collects the resolvable paths of every input WAD and writes them to a single
/// hashtable in the requested format.
pub fn rip_paths(args: PathsArgs, hashtable: &WadHashtable) -> eyre::Result<()> {
    let format = resolve_format(args.format, args.output.as_deref());
    let output = resolve_output_path(args.output.as_deref(), &args.inputs, format)?;

    let filter_pattern = create_filter_pattern(args.pattern)?;

    // Deduplicated across every input WAD; keyed by chunk hash so a path shared by
    // multiple WADs is written once.
    let mut collected: BTreeMap<u64, Arc<str>> = BTreeMap::new();
    let mut recovered_total = 0usize;

    for input in &args.inputs {
        let source = File::open(input).wrap_err_with(|| format!("failed to open WAD '{input}'"))?;
        let mut wad =
            Wad::mount(source).wrap_err_with(|| format!("failed to mount WAD '{input}'"))?;

        let chunk_hashes: HashSet<u64> = wad.chunks().iter().map(|c| c.path_hash).collect();

        let discovered = if args.resolve_bin_paths {
            scan_wad_bin_paths(&mut wad, hashtable, &chunk_hashes, args.full_bin_scan)
        } else {
            HashMap::new()
        };
        recovered_total += discovered.len();

        for &hash in &chunk_hashes {
            // Prefer bin-recovered names, then fall back to the shared cache. A chunk
            // that neither source resolves would only be the 16-hex fallback - skip it.
            let path: Arc<str> = if let Some(path) = discovered.get(&hash) {
                path.clone()
            } else if hashtable.contains(hash) {
                Arc::from(hashtable.resolve_path(hash).as_ref())
            } else {
                continue;
            };

            if should_skip_pattern(&path, filter_pattern.as_ref(), args.filter_invert) {
                continue;
            }

            collected.entry(hash).or_insert(path);
        }
    }

    if collected.is_empty() {
        tracing::warn!(
            "no resolvable paths found in the given WAD(s); nothing written to {}",
            output
        );
        return Ok(());
    }

    let write_stats = match format {
        PathsFormat::Txt => {
            write_txt(&output, &collected)?;
            None
        }
        PathsFormat::Lhdb => Some(write_lhdb(&output, &collected)?),
    };

    if args.show_stats {
        print_stats(&output, format, &collected, recovered_total, write_stats);
    } else {
        tracing::info!("wrote {} paths to {}", collected.len(), output);
    }

    Ok(())
}

/// Resolves the output format: an explicit `--format` wins; otherwise a `.lhdb`
/// output extension selects the table format, and everything else is text.
fn resolve_format(explicit: Option<PathsFormat>, output: Option<&str>) -> PathsFormat {
    if let Some(format) = explicit {
        return format;
    }
    let is_lhdb = output
        .map(Utf8Path::new)
        .and_then(Utf8Path::extension)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lhdb"));
    if is_lhdb {
        PathsFormat::Lhdb
    } else {
        PathsFormat::Txt
    }
}

/// Derives the output file path when the user did not pass `-o`.
///
/// A single input writes a sibling `<stem>.paths.<ext>` next to the WAD (so the
/// Explorer right-click flow drops the hashtable beside the file it was invoked on);
/// multiple inputs collapse into `wadtools-paths.<ext>` in the current directory.
fn resolve_output_path(
    explicit: Option<&str>,
    inputs: &[String],
    format: PathsFormat,
) -> eyre::Result<String> {
    if let Some(path) = explicit {
        return Ok(path.to_string());
    }

    let ext = format.extension();
    match inputs {
        [single] => {
            let input = Utf8Path::new(single);
            let parent = input.parent().unwrap_or(Utf8Path::new("."));
            let stem = input.file_stem().unwrap_or("wadtools");
            Ok(parent.join(format!("{stem}.paths.{ext}")).to_string())
        }
        _ => Ok(format!("wadtools-paths.{ext}")),
    }
}

/// Writes the CDragon hashtable format (`<hex-hash> <path>` text), sorted by path
/// for readable, deterministic diffs.
fn write_txt(output: &str, collected: &BTreeMap<u64, Arc<str>>) -> eyre::Result<()> {
    let mut entries: Vec<(&u64, &Arc<str>)> = collected.iter().collect();
    entries.sort_by(|a, b| a.1.cmp(b.1));

    let file = File::create(output)
        .wrap_err_with(|| format!("failed to create output file '{output}'"))?;
    let mut writer = BufWriter::new(file);
    for (hash, path) in entries {
        writeln!(writer, "{} {}", format_chunk_path_hash(*hash), path)?;
    }
    writer.flush()?;
    Ok(())
}

/// Writes a mimir `.lhdb` hash table in the Game-table configuration so it layers
/// cleanly on top of the shared cache (XXH64 keys over the lowercased path).
fn write_lhdb(
    output: &str,
    collected: &BTreeMap<u64, Arc<str>>,
) -> eyre::Result<ltk_hashdb::BuildStats> {
    let mut writer = HashDbWriter::new(KeyWidth::U64, Compression::default())
        .hash_kind(HashKind::Xxh64)
        .casing(Casing::Insensitive);
    for (&hash, path) in collected {
        writer.insert(hash, path);
    }

    let file = File::create(output)
        .wrap_err_with(|| format!("failed to create output file '{output}'"))?;
    let stats = writer
        .build(file)
        .wrap_err_with(|| format!("failed to build .lhdb table '{output}'"))?;
    Ok(stats)
}

fn print_stats(
    output: &str,
    format: PathsFormat,
    collected: &BTreeMap<u64, Arc<str>>,
    recovered_total: usize,
    build_stats: Option<ltk_hashdb::BuildStats>,
) {
    println!();
    println!(
        "{}: {} ({})",
        "Output".bright_cyan().bold(),
        output.bright_white(),
        format.extension().bright_magenta()
    );
    println!(
        "{}: {}",
        "Paths".bright_cyan().bold(),
        collected.len().to_string().bright_green()
    );
    if recovered_total > 0 {
        println!(
            "{}: {} names from bins",
            "Recovered".bright_cyan().bold(),
            recovered_total.to_string().bright_green()
        );
    }
    if let Some(stats) = build_stats {
        println!(
            "{}: {} entries, arena {} → {} bytes",
            "Table".bright_cyan().bold(),
            stats.entries.to_string().bright_white(),
            stats.arena_decompressed_size.to_string().bright_white(),
            stats.arena_compressed_size.to_string().bright_green()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_scan::hash_wad_path;
    use league_toolkit::meta::BinTree;
    use league_toolkit::wad::{WadBuilder, WadChunkBuilder};
    use ltk_hashdb::HashDb;
    use std::io::{Cursor, Write};

    // --- pure helpers ---

    #[test]
    fn format_defaults_to_txt_and_infers_lhdb_from_extension() {
        assert_eq!(resolve_format(None, None), PathsFormat::Txt);
        assert_eq!(resolve_format(None, Some("out.txt")), PathsFormat::Txt);
        assert_eq!(resolve_format(None, Some("out.lhdb")), PathsFormat::Lhdb);
        assert_eq!(resolve_format(None, Some("out.LHDB")), PathsFormat::Lhdb);
        // Explicit format always wins over the extension.
        assert_eq!(
            resolve_format(Some(PathsFormat::Txt), Some("out.lhdb")),
            PathsFormat::Txt
        );
    }

    #[test]
    fn single_input_derives_sibling_paths_file() {
        // `camino` joins with the platform separator, so normalize before comparing.
        let normalize = |s: String| s.replace('\\', "/");
        let inputs = vec!["some/dir/Aatrox.wad.client".to_string()];

        let out = resolve_output_path(None, &inputs, PathsFormat::Txt).unwrap();
        assert_eq!(normalize(out), "some/dir/Aatrox.wad.paths.txt");

        let out = resolve_output_path(None, &inputs, PathsFormat::Lhdb).unwrap();
        assert_eq!(normalize(out), "some/dir/Aatrox.wad.paths.lhdb");
    }

    #[test]
    fn multiple_inputs_default_to_a_single_combined_file() {
        let inputs = vec!["a.wad".to_string(), "b.wad".to_string()];
        let out = resolve_output_path(None, &inputs, PathsFormat::Txt).unwrap();
        assert_eq!(out, "wadtools-paths.txt");
    }

    #[test]
    fn explicit_output_is_used_verbatim() {
        let inputs = vec!["a.wad".to_string()];
        let out = resolve_output_path(Some("custom/out.txt"), &inputs, PathsFormat::Txt).unwrap();
        assert_eq!(out, "custom/out.txt");
    }

    // --- end-to-end over a real on-disk WAD ---

    /// A WAD holding a known `.bin` that references an otherwise-anonymous asset,
    /// plus an unreferenced anonymous chunk. Returns `(wad_path, hashtable, hashes)`.
    struct Fixture {
        wad_path: std::path::PathBuf,
        hashtable: WadHashtable,
        bin_hash: u64,
        asset_hash: u64,
        unknown_hash: u64,
    }

    fn build_fixture(tag: &str) -> Fixture {
        let bin_path = "data/test.bin";
        let asset_path = "assets/characters/foo/recovered.dds";
        let unknown_path = "assets/unknown/orphan.dds";

        // The known bin links to the anonymous asset, so a scan recovers its name.
        let bin_bytes = {
            let tree = BinTree::builder().dependency(asset_path).build();
            let mut buffer = Cursor::new(Vec::new());
            tree.to_writer(&mut buffer).unwrap();
            buffer.into_inner()
        };

        let bin_hash = hash_wad_path(bin_path);
        let asset_hash = hash_wad_path(asset_path);
        let unknown_hash = hash_wad_path(unknown_path);

        let wad_path = std::env::temp_dir().join(format!(
            "wadtools_paths_{tag}_{}.wad.client",
            std::process::id()
        ));
        let mut wad_buffer = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(WadChunkBuilder::default().with_path(bin_path))
            .with_chunk(WadChunkBuilder::default().with_path(asset_path))
            .with_chunk(WadChunkBuilder::default().with_path(unknown_path))
            .build_to_writer(&mut wad_buffer, |path_hash, cursor| {
                if path_hash == bin_hash {
                    cursor.write_all(&bin_bytes)?;
                } else {
                    cursor.write_all(&[0xAB; 32])?;
                }
                Ok(())
            })
            .unwrap();
        std::fs::write(&wad_path, wad_buffer.into_inner()).unwrap();

        // The cache knows only the bin's name; the asset is recovered via bin scan and
        // the orphan stays anonymous (hex-only) and must be dropped from the output.
        let mut hashtable = WadHashtable::new().unwrap();
        hashtable.insert(bin_hash, bin_path);

        Fixture {
            wad_path,
            hashtable,
            bin_hash,
            asset_hash,
            unknown_hash,
        }
    }

    fn paths_args(wad_path: &std::path::Path, output: &str, format: PathsFormat) -> PathsArgs {
        PathsArgs {
            inputs: vec![wad_path.to_str().unwrap().to_string()],
            output: Some(output.to_string()),
            format: Some(format),
            pattern: None,
            filter_invert: false,
            resolve_bin_paths: true,
            full_bin_scan: false,
            show_stats: false,
        }
    }

    #[test]
    fn txt_output_contains_known_and_recovered_and_drops_unknown() {
        let fx = build_fixture("txt");
        let out =
            std::env::temp_dir().join(format!("wadtools_paths_txt_{}.txt", std::process::id()));
        let out_str = out.to_str().unwrap();

        rip_paths(
            paths_args(&fx.wad_path, out_str, PathsFormat::Txt),
            &fx.hashtable,
        )
        .unwrap();

        let contents = std::fs::read_to_string(&out).unwrap();
        assert!(
            contents.contains("data/test.bin"),
            "known bin path missing: {contents}"
        );
        assert!(
            contents.contains("assets/characters/foo/recovered.dds"),
            "recovered asset path missing: {contents}"
        );
        // The anonymous orphan resolves only to its hex fallback and must be skipped.
        assert!(
            !contents.contains(&format_chunk_path_hash(fx.unknown_hash)),
            "hex-only orphan should not be written: {contents}"
        );
        // Each written line is `<hex-hash> <path>`, matching the -H hashtable format.
        assert!(contents.contains(&format!(
            "{} data/test.bin",
            format_chunk_path_hash(fx.bin_hash)
        )));

        let _ = std::fs::remove_file(&fx.wad_path);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn lhdb_output_is_a_valid_readable_table() {
        let fx = build_fixture("lhdb");
        let out =
            std::env::temp_dir().join(format!("wadtools_paths_lhdb_{}.lhdb", std::process::id()));

        rip_paths(
            paths_args(&fx.wad_path, out.to_str().unwrap(), PathsFormat::Lhdb),
            &fx.hashtable,
        )
        .unwrap();

        // The produced table must be openable and resolve exactly the two known paths,
        // by the same XXH64 chunk hashes the WAD keys on.
        let db = HashDb::open(&out).unwrap();
        assert_eq!(
            db.get(fx.bin_hash).as_deref(),
            Some("data/test.bin"),
            "bin path not resolvable from the .lhdb"
        );
        assert_eq!(
            db.get(fx.asset_hash).as_deref(),
            Some("assets/characters/foo/recovered.dds"),
            "recovered asset not resolvable from the .lhdb"
        );
        assert!(
            !db.contains(fx.unknown_hash),
            "hex-only orphan should not be present in the .lhdb"
        );

        drop(db);
        let _ = std::fs::remove_file(&fx.wad_path);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn pattern_filter_limits_the_written_paths() {
        let fx = build_fixture("filter");
        let out =
            std::env::temp_dir().join(format!("wadtools_paths_filter_{}.txt", std::process::id()));

        let mut args = paths_args(&fx.wad_path, out.to_str().unwrap(), PathsFormat::Txt);
        args.pattern = Some(r"\.dds$".to_string());

        rip_paths(args, &fx.hashtable).unwrap();

        let contents = std::fs::read_to_string(&out).unwrap();
        assert!(contents.contains("assets/characters/foo/recovered.dds"));
        assert!(
            !contents.contains("data/test.bin"),
            "pattern should have excluded the .bin: {contents}"
        );

        let _ = std::fs::remove_file(&fx.wad_path);
        let _ = std::fs::remove_file(&out);
    }
}
