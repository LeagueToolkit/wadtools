use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek},
};

use colored::Colorize;
use fancy_regex::Regex;
use league_toolkit::wad::{Wad, WadChunk, WadHash};
use serde::Serialize;
use std::borrow::Cow;

use crate::{
    filters::{should_skip_hash, should_skip_pattern},
    utils::{format_chunk_path_hash, format_size, WadHashtable},
};

/// A difference between two WAD chunks
enum ChunkDiff {
    /// A new chunk in the target WAD
    New(WadChunk),
    /// A removed chunk in the target WAD
    Removed(WadChunk),
    /// A modified chunk in the target WAD
    Modified { old: WadChunk, new: WadChunk },
    /// A renamed chunk in the target WAD
    Renamed { old: WadChunk, new: WadChunk },
}

/// A record for a chunk diff in a CSV file
#[derive(Debug, Serialize)]
struct ChunkDiffCsvRecord {
    diff_type: String,
    hash: String,
    path: String,
    new_path: String,
    old_uncompressed_size: usize,
    new_uncompressed_size: usize,
    old_compressed_size: usize,
    new_compressed_size: usize,
    old_compression_type: String,
    new_compression_type: String,
}

pub struct DiffArgs {
    pub reference: String,
    pub target: String,
    pub output: Option<String>,
    pub pattern: Option<String>,
    pub hash: Option<Vec<WadHash>>,
    pub filter_invert: bool,
}

pub fn diff(args: DiffArgs, hashtable: &WadHashtable) -> eyre::Result<()> {
    let reference_wad_file = File::open(&args.reference)?;
    let target_wad_file = File::open(&args.target)?;

    let reference_wad = Wad::mount(reference_wad_file)?;
    let target_wad = Wad::mount(target_wad_file)?;

    tracing::info!("Collecting diffs...");
    let diffs = collect_diffs(&reference_wad, &target_wad);

    let filter_pattern = crate::utils::create_filter_pattern(args.pattern)?;

    if let Some(output_path) = args.output {
        write_diffs_to_csv(
            &diffs,
            hashtable,
            &output_path,
            filter_pattern.as_ref(),
            args.hash.as_deref(),
            args.filter_invert,
        )?;
    } else {
        print_diffs(
            &diffs,
            hashtable,
            filter_pattern.as_ref(),
            args.hash.as_deref(),
            args.filter_invert,
        );
    }

    Ok(())
}

/// Returns the primary path hash for a diff entry (used for filtering).
fn diff_primary_path_hash(diff: &ChunkDiff) -> WadHash {
    match diff {
        ChunkDiff::New(chunk) => chunk.path_hash,
        ChunkDiff::Removed(chunk) => chunk.path_hash,
        ChunkDiff::Modified { old, .. } => old.path_hash,
        ChunkDiff::Renamed { new, .. } => new.path_hash,
    }
}

/// Returns the primary resolved path for a diff entry (used for sorting/filtering).
fn diff_primary_path<'a>(diff: &ChunkDiff, hashtable: &'a WadHashtable) -> Cow<'a, str> {
    match diff {
        ChunkDiff::New(chunk) => hashtable.resolve_path(chunk.path_hash),
        ChunkDiff::Removed(chunk) => hashtable.resolve_path(chunk.path_hash),
        ChunkDiff::Modified { old, .. } => hashtable.resolve_path(old.path_hash),
        ChunkDiff::Renamed { new, .. } => hashtable.resolve_path(new.path_hash),
    }
}

/// Returns true if a diff entry should be skipped based on filters.
fn should_skip_diff(
    diff: &ChunkDiff,
    hashtable: &WadHashtable,
    filter_pattern: Option<&Regex>,
    hash_filter: Option<&[WadHash]>,
    filter_invert: bool,
) -> bool {
    let path_hash = diff_primary_path_hash(diff);
    if should_skip_hash(path_hash, hash_filter, filter_invert) {
        return true;
    }
    let path = diff_primary_path(diff, hashtable);
    if should_skip_pattern(path.as_ref(), filter_pattern, filter_invert) {
        return true;
    }
    false
}

fn print_diffs(
    diffs: &[ChunkDiff],
    hashtable: &WadHashtable,
    filter_pattern: Option<&Regex>,
    hash_filter: Option<&[WadHash]>,
    filter_invert: bool,
) {
    // Sort by resolved path
    let mut sorted_indices: Vec<usize> = (0..diffs.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let path_a = diff_primary_path(&diffs[a], hashtable);
        let path_b = diff_primary_path(&diffs[b], hashtable);
        path_a.cmp(&path_b)
    });

    let mut count_new: usize = 0;
    let mut count_removed: usize = 0;
    let mut count_modified: usize = 0;
    let mut count_renamed: usize = 0;

    for &idx in &sorted_indices {
        let diff = &diffs[idx];

        if should_skip_diff(diff, hashtable, filter_pattern, hash_filter, filter_invert) {
            continue;
        }

        match diff {
            ChunkDiff::New(chunk) => {
                let path = hashtable.resolve_path(chunk.path_hash);
                println!("{} {}", "+".bright_green(), path.bright_green());
                count_new += 1;
            }
            ChunkDiff::Removed(chunk) => {
                let path = hashtable.resolve_path(chunk.path_hash);
                println!("{} {}", "-".bright_red(), path.bright_red());
                count_removed += 1;
            }
            ChunkDiff::Modified { old, new } => {
                let path = hashtable.resolve_path(old.path_hash);
                let old_size = format_size(old.uncompressed_size as u64);
                let new_size = format_size(new.uncompressed_size as u64);

                let mut detail = format!("({} → {})", old_size, new_size);

                if old.compression_type != new.compression_type {
                    detail = format!(
                        "({} → {}, {} → {})",
                        old_size, new_size, old.compression_type, new.compression_type
                    );
                }

                println!(
                    "{} {}  {}",
                    "~".bright_yellow(),
                    path.bright_yellow(),
                    detail.bright_black()
                );
                count_modified += 1;
            }
            ChunkDiff::Renamed { old, new } => {
                let old_path = hashtable.resolve_path(old.path_hash);
                let new_path = hashtable.resolve_path(new.path_hash);
                println!(
                    "{} {} -> {}",
                    ">".bright_cyan(),
                    old_path.bright_blue(),
                    new_path.bright_cyan()
                );
                count_renamed += 1;
            }
        }
    }

    let total = count_new + count_removed + count_modified + count_renamed;
    if total > 0 {
        println!();
        println!(
            "Summary: {} new, {} removed, {} modified, {} renamed",
            format!("+{}", count_new).bright_green(),
            format!("-{}", count_removed).bright_red(),
            format!("~{}", count_modified).bright_yellow(),
            format!(">{}", count_renamed).bright_cyan(),
        );
    }
}

fn collect_diffs<TRefSource, TTargetSource>(
    reference_wad: &Wad<TRefSource>,
    target_wad: &Wad<TTargetSource>,
) -> Vec<ChunkDiff>
where
    TRefSource: Read + Seek,
    TTargetSource: Read + Seek,
{
    let mut diffs = Vec::<ChunkDiff>::new();

    for reference_chunk in reference_wad.chunks() {
        let target_chunk = target_wad.chunks().get(reference_chunk.path_hash);

        // If the chunk is not present in the target wad, it is a removed chunk
        if target_chunk.is_none() {
            diffs.push(ChunkDiff::Removed(*reference_chunk));
        }

        // If the chunk is present in the target wad, we need to compare the two chunks
        if let Some(target_chunk) = target_chunk {
            if target_chunk.checksum != reference_chunk.checksum {
                diffs.push(ChunkDiff::Modified {
                    old: *reference_chunk,
                    new: *target_chunk,
                });
            }
        }
    }

    // Collect renamed hashes so we can filter out stale Removed entries
    let mut renamed_old_hashes = Vec::new();

    for target_chunk in target_wad.chunks() {
        let reference_chunk = reference_wad.chunks().get(target_chunk.path_hash);

        // If the chunk is not present in the reference wad, it is either a new chunk or a renamed chunk
        if reference_chunk.is_none() {
            // We can check if the chunk is renamed, by finding a chunk in the reference wad with the same checksum
            let renamed_chunk = reference_wad
                .chunks()
                .iter()
                .find(|chunk| chunk.checksum == target_chunk.checksum);

            if let Some(renamed_chunk) = renamed_chunk {
                renamed_old_hashes.push(renamed_chunk.path_hash);
                diffs.push(ChunkDiff::Renamed {
                    old: *renamed_chunk,
                    new: *target_chunk,
                });
            } else {
                diffs.push(ChunkDiff::New(*target_chunk));
            }
        }
    }

    // Filter out Removed entries whose path_hash matches a Renamed entry's old.path_hash
    if !renamed_old_hashes.is_empty() {
        diffs.retain(|diff| {
            if let ChunkDiff::Removed(chunk) = diff {
                !renamed_old_hashes.contains(&chunk.path_hash)
            } else {
                true
            }
        });
    }

    diffs
}

fn write_diffs_to_csv(
    diffs: &[ChunkDiff],
    hashtable: &WadHashtable,
    output_path: &str,
    filter_pattern: Option<&Regex>,
    hash_filter: Option<&[WadHash]>,
    filter_invert: bool,
) -> eyre::Result<()> {
    tracing::info!("Writing diffs to CSV file: {}", output_path.bright_cyan());

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(output_path)?;

    let mut writer = csv::Writer::from_writer(file);
    let mut records =
        create_csv_records(diffs, hashtable, filter_pattern, hash_filter, filter_invert);

    records.sort_by(|a, b| a.path.cmp(&b.path));

    for record in records.iter() {
        writer.serialize(record)?;
    }

    writer.flush()?;

    tracing::info!("Wrote {} diffs to CSV file", records.len());
    Ok(())
}

fn create_csv_records(
    diffs: &[ChunkDiff],
    hashtable: &WadHashtable,
    filter_pattern: Option<&Regex>,
    hash_filter: Option<&[WadHash]>,
    filter_invert: bool,
) -> Vec<ChunkDiffCsvRecord> {
    let mut records = Vec::<ChunkDiffCsvRecord>::new();
    for diff in diffs {
        if should_skip_diff(diff, hashtable, filter_pattern, hash_filter, filter_invert) {
            continue;
        }

        match diff {
            ChunkDiff::New(chunk) => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "new".to_string(),
                    hash: format_chunk_path_hash(chunk.path_hash),
                    path: hashtable.resolve_path(chunk.path_hash).to_string(),
                    new_path: "".to_string(),
                    old_uncompressed_size: 0,
                    new_uncompressed_size: chunk.uncompressed_size,
                    old_compressed_size: 0,
                    new_compressed_size: chunk.compressed_size,
                    old_compression_type: "".to_string(),
                    new_compression_type: chunk.compression_type.to_string(),
                });
            }
            ChunkDiff::Removed(chunk) => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "removed".to_string(),
                    hash: format_chunk_path_hash(chunk.path_hash),
                    path: hashtable.resolve_path(chunk.path_hash).to_string(),
                    new_path: "".to_string(),
                    old_uncompressed_size: chunk.uncompressed_size,
                    new_uncompressed_size: 0,
                    old_compressed_size: chunk.compressed_size,
                    new_compressed_size: 0,
                    old_compression_type: chunk.compression_type.to_string(),
                    new_compression_type: "".to_string(),
                });
            }
            ChunkDiff::Modified { old, new } => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "modified".to_string(),
                    hash: format_chunk_path_hash(old.path_hash),
                    path: hashtable.resolve_path(old.path_hash).to_string(),
                    new_path: "".to_string(),
                    old_uncompressed_size: old.uncompressed_size,
                    new_uncompressed_size: new.uncompressed_size,
                    old_compressed_size: old.compressed_size,
                    new_compressed_size: new.compressed_size,
                    old_compression_type: old.compression_type.to_string(),
                    new_compression_type: new.compression_type.to_string(),
                });
            }
            ChunkDiff::Renamed { old, new } => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "renamed".to_string(),
                    hash: format_chunk_path_hash(old.path_hash),
                    path: hashtable.resolve_path(old.path_hash).to_string(),
                    new_path: hashtable.resolve_path(new.path_hash).to_string(),
                    old_uncompressed_size: old.uncompressed_size,
                    new_uncompressed_size: new.uncompressed_size,
                    old_compressed_size: old.compressed_size,
                    new_compressed_size: new.compressed_size,
                    old_compression_type: old.compression_type.to_string(),
                    new_compression_type: new.compression_type.to_string(),
                });
            }
        }
    }

    records
}
