use camino::Utf8Path;
use color_eyre::owo_colors::OwoColorize;
use league_toolkit::{file::LeagueFileKind, wad::Wad};
use serde::Serialize;
use std::fs::File;

use crate::utils::{
    create_filter_pattern, default_hashtable_dir, format_chunk_path_hash, WadHashtable,
};

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ListOutputFormat {
    /// Default human-readable table format
    #[default]
    Table,
    /// JSON output for scripting
    Json,
    /// CSV output for spreadsheets
    Csv,
    /// Simple flat list of paths only
    Flat,
}

pub struct ListArgs {
    pub input: String,
    pub hashtable: Option<String>,
    pub hashtable_dir: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
    pub pattern: Option<String>,
    pub format: ListOutputFormat,
    pub show_stats: bool,
}

#[derive(Debug, Serialize)]
struct ChunkInfo {
    hash: String,
    path: String,
    file_type: String,
    compressed_size: usize,
    uncompressed_size: usize,
    compression_ratio: f64,
}

#[derive(Debug, Serialize)]
struct ListOutput {
    wad_path: String,
    total_chunks: usize,
    matched_chunks: usize,
    total_compressed_size: u64,
    total_uncompressed_size: u64,
    overall_compression_ratio: f64,
    chunks: Vec<ChunkInfo>,
}

pub fn list(args: ListArgs) -> eyre::Result<()> {
    let source = File::open(&args.input)?;
    let wad = Wad::mount(&source)?;

    // Load hashtables
    let mut hashtable = WadHashtable::new()?;
    if let Some(dir_override) = &args.hashtable_dir {
        hashtable.add_from_dir(Utf8Path::new(dir_override))?;
    } else if let Some(dir) = default_hashtable_dir() {
        hashtable.add_from_dir(dir)?;
    }
    if let Some(hashtable_path) = &args.hashtable {
        tracing::info!("loading hashtable from {}", hashtable_path);
        hashtable.add_from_file(&File::open(hashtable_path)?)?;
    }

    // Build filter pattern
    let filter_pattern = create_filter_pattern(args.pattern)?;

    // Collect chunk info
    let mut chunks_info: Vec<ChunkInfo> = Vec::new();
    let mut total_compressed: u64 = 0;
    let mut total_uncompressed: u64 = 0;

    for chunk in wad.chunks().values() {
        let path_str = hashtable.resolve_path(chunk.path_hash);

        // Apply pattern filter
        if let Some(ref regex) = filter_pattern {
            if !regex.is_match(path_str.as_ref()).unwrap_or(false) {
                continue;
            }
        }

        // Detect file type from extension or guess from path
        let file_type = LeagueFileKind::from_extension(
            Utf8Path::new(path_str.as_ref()).extension().unwrap_or(""),
        );

        // Apply type filter
        if let Some(ref filter_types) = args.filter_type {
            if !filter_types.contains(&file_type) {
                continue;
            }
        }

        let compressed = chunk.compressed_size;
        let uncompressed = chunk.uncompressed_size;
        let ratio = if uncompressed > 0 {
            (1.0 - (compressed as f64 / uncompressed as f64)) * 100.0
        } else {
            0.0
        };

        total_compressed += compressed as u64;
        total_uncompressed += uncompressed as u64;

        chunks_info.push(ChunkInfo {
            hash: format_chunk_path_hash(chunk.path_hash),
            path: path_str.to_string(),
            file_type: format!("{:?}", file_type),
            compressed_size: compressed,
            uncompressed_size: uncompressed,
            compression_ratio: ratio,
        });
    }

    // Sort by path for consistent output
    chunks_info.sort_by(|a, b| a.path.cmp(&b.path));

    let overall_ratio = if total_uncompressed > 0 {
        (1.0 - (total_compressed as f64 / total_uncompressed as f64)) * 100.0
    } else {
        0.0
    };

    let output = ListOutput {
        wad_path: args.input.clone(),
        total_chunks: wad.chunks().len(),
        matched_chunks: chunks_info.len(),
        total_compressed_size: total_compressed,
        total_uncompressed_size: total_uncompressed,
        overall_compression_ratio: overall_ratio,
        chunks: chunks_info,
    };

    match args.format {
        ListOutputFormat::Json => print_json(&output)?,
        ListOutputFormat::Csv => print_csv(&output)?,
        ListOutputFormat::Flat => print_flat(&output),
        ListOutputFormat::Table => print_table(&output, args.show_stats),
    }

    Ok(())
}

fn print_json(output: &ListOutput) -> eyre::Result<()> {
    println!("{}", serde_json::to_string_pretty(output)?);
    Ok(())
}

fn print_csv(output: &ListOutput) -> eyre::Result<()> {
    let mut writer = csv::Writer::from_writer(std::io::stdout());

    for chunk in &output.chunks {
        writer.serialize(chunk)?;
    }

    writer.flush()?;
    Ok(())
}

fn print_flat(output: &ListOutput) {
    for chunk in &output.chunks {
        println!("{}", chunk.path);
    }
}

fn print_table(output: &ListOutput, show_stats: bool) {
    // Print header
    if show_stats {
        println!(
            "{}: {}",
            "WAD".bright_cyan().bold(),
            output.wad_path.bright_white()
        );
        println!(
            "{}: {} ({} matched)",
            "Chunks".bright_cyan().bold(),
            output.total_chunks.to_string().bright_yellow(),
            output.matched_chunks.to_string().bright_green()
        );
        println!(
            "{}: {} → {} ({:.1}% saved)",
            "Size".bright_cyan().bold(),
            format_size(output.total_uncompressed_size).bright_white(),
            format_size(output.total_compressed_size).bright_green(),
            output.overall_compression_ratio
        );
        println!();
    }

    // Column headers
    println!(
        "{:>12} {:>12} {:>7} {:16}  {}",
        "COMPRESSED".bright_blue().bold(),
        "SIZE".bright_blue().bold(),
        "RATIO".bright_blue().bold(),
        "TYPE".bright_blue().bold(),
        "PATH".bright_blue().bold()
    );
    println!("{}", "─".repeat(80).bright_black());

    for chunk in &output.chunks {
        // Format the ratio as a fixed-width string first, then apply color
        let ratio_str = format!("{:>6.1}%", chunk.compression_ratio);
        let ratio_colored = if chunk.compression_ratio > 50.0 {
            ratio_str.bright_green().to_string()
        } else if chunk.compression_ratio > 20.0 {
            ratio_str.bright_yellow().to_string()
        } else {
            ratio_str.bright_red().to_string()
        };

        println!(
            "{:>12} {:>12} {} {:16}  {}",
            format_size(chunk.compressed_size as u64).bright_black(),
            format_size(chunk.uncompressed_size as u64),
            ratio_colored,
            chunk.file_type.bright_magenta(),
            chunk.path
        );
    }

    if show_stats {
        println!("{}", "─".repeat(80).bright_black());
        println!(
            "{} chunks listed",
            output.matched_chunks.to_string().bright_green().bold()
        );
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
