use camino::Utf8Path;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek},
};

use colored::Colorize;
use league_toolkit::wad::{Wad, WadChunk};
use serde::Serialize;

use crate::utils::{default_hashtable_dir, format_chunk_path_hash, WadHashtable};

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
}

pub struct DiffArgs {
    pub reference: String,
    pub target: String,
    pub hashtable_path: Option<String>,
    pub output: Option<String>,
    pub hashtable_dir: Option<String>,
}

pub fn diff(args: DiffArgs) -> eyre::Result<()> {
    let reference_wad_file = File::open(&args.reference)?;
    let target_wad_file = File::open(&args.target)?;

    let mut hashtable = WadHashtable::new()?;
    if let Some(dir_override) = &args.hashtable_dir {
        hashtable.add_from_dir(Utf8Path::new(dir_override))?;
    } else if let Some(dir) = default_hashtable_dir() {
        hashtable.add_from_dir(dir)?;
    }
    if let Some(hashtable_path) = args.hashtable_path {
        hashtable.add_from_file(&File::open(&hashtable_path)?)?;
    }

    let reference_wad = Wad::mount(&reference_wad_file)?;
    let target_wad = Wad::mount(&target_wad_file)?;

    tracing::info!("Collecting diffs...");
    let diffs = collect_diffs(&reference_wad, &target_wad);

    if let Some(output_path) = args.output {
        write_diffs_to_csv(&diffs, &hashtable, &output_path)?;
    } else {
        print_diffs(&diffs, &hashtable);
    }

    Ok(())
}

fn print_diffs(diffs: &[ChunkDiff], hashtable: &WadHashtable) {
    for diff in diffs {
        match diff {
            ChunkDiff::New(chunk) => {
                let path = hashtable.resolve_path(chunk.path_hash);

                println!("+ {}", path.bright_green());
            }
            ChunkDiff::Removed(chunk) => {
                let path = hashtable.resolve_path(chunk.path_hash);

                println!("- {}", path.bright_red());
            }
            ChunkDiff::Modified { old, new: _ } => {
                let path = hashtable.resolve_path(old.path_hash);

                // For modified chunks, we print the chunk path in yellow, and somehow also print the new file sizes
                println!("! {}", path.bright_yellow());
            }
            ChunkDiff::Renamed { old, new } => {
                let old_path = hashtable.resolve_path(old.path_hash);
                let new_path = hashtable.resolve_path(new.path_hash);

                println!("! {} -> {}", old_path.bright_blue(), new_path.bright_cyan());
            }
        }
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

    for (reference_chunk_hash, reference_chunk) in reference_wad.chunks() {
        let target_chunk = target_wad.chunks().get(reference_chunk_hash);

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

    for (target_chunk_hash, target_chunk) in target_wad.chunks() {
        let reference_chunk = reference_wad.chunks().get(target_chunk_hash);

        // If the chunk is not present in the reference wad, it is either a new chunk or a renamed chunk
        if reference_chunk.is_none() {
            // We can check if the chunk is renamed, by finding a chunk in the reference wad with the same checksum
            let renamed_chunk = reference_wad
                .chunks()
                .values()
                .find(|chunk| chunk.checksum == target_chunk.checksum);

            if let Some(renamed_chunk) = renamed_chunk {
                diffs.push(ChunkDiff::Renamed {
                    old: *renamed_chunk,
                    new: *target_chunk,
                });
            } else {
                diffs.push(ChunkDiff::New(*target_chunk));
            }
        }
    }

    diffs
}

fn write_diffs_to_csv(
    diffs: &[ChunkDiff],
    hashtable: &WadHashtable,
    output_path: &str,
) -> eyre::Result<()> {
    tracing::info!("Writing diffs to CSV file: {}", output_path.bright_cyan());

    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(output_path)?;

    let mut writer = csv::Writer::from_writer(file);
    let mut records = create_csv_records(diffs, hashtable);

    records.sort_by(|a, b| a.path.cmp(&b.path));

    for record in records.iter() {
        writer.serialize(record)?;
    }

    writer.flush()?;

    tracing::info!("Wrote {} diffs to CSV file", records.len());
    Ok(())
}

fn create_csv_records(diffs: &[ChunkDiff], hashtable: &WadHashtable) -> Vec<ChunkDiffCsvRecord> {
    let mut records = Vec::<ChunkDiffCsvRecord>::new();
    for diff in diffs {
        match diff {
            ChunkDiff::New(chunk) => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "new".to_string(),
                    hash: format_chunk_path_hash(chunk.path_hash),
                    path: hashtable.resolve_path(chunk.path_hash).to_string(),
                    new_path: "".to_string(),
                    old_uncompressed_size: chunk.uncompressed_size,
                    new_uncompressed_size: chunk.uncompressed_size,
                });
            }
            ChunkDiff::Removed(chunk) => {
                records.push(ChunkDiffCsvRecord {
                    diff_type: "removed".to_string(),
                    hash: format_chunk_path_hash(chunk.path_hash),
                    path: hashtable.resolve_path(chunk.path_hash).to_string(),
                    new_path: "".to_string(),
                    old_uncompressed_size: chunk.uncompressed_size,
                    new_uncompressed_size: chunk.uncompressed_size,
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
                });
            }
        }
    }

    records
}
