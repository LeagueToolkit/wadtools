use clap::{Parser, Subcommand};
use league_toolkit::file::LeagueFileKind;
use tracing::Level;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter, fmt};
mod commands;
mod extractor;
mod utils;

use commands::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Extract the contents of a wad file
    #[command(visible_alias = "e")]
    Extract {
        /// Path to the input wad file
        #[arg(short, long)]
        input: String,

        /// Path to the output directory
        #[arg(short, long)]
        output: String,

        /// Path to the hashtable file
        #[arg(short = 'H', long, visible_short_alias = 'd')]
        hashtable: Option<String>,

        #[arg(
            short = 'f',
            long,
            value_name = "FILTER_MAGIC",
            help = "Filter files by magic (e.g., 'png', 'bin'). You can pass multiple values at once.",
            value_parser = parse_filter_type,
            num_args = 1..
        )]
        filter_type: Option<Vec<LeagueFileKind>>,

        /// Only extract chunks whose resolved path matches this regex
        #[arg(
            short = 'x',
            long,
            value_name = "REGEX",
            help = "Only extract chunks whose resolved path matches this regex (case-insensitive by default; use (?-i) to disable)"
        )]
        pattern: Option<String>,
    },
    /// Compare two wad files
    ///
    /// This command compares two wad files and prints the differences between them.
    /// Using the reference wad file, it will print the differences between the target wad file.
    ///
    #[command(visible_alias = "d")]
    Diff {
        /// Path to the reference wad file
        #[arg(short, long)]
        reference: String,

        /// Path to the target wad file
        #[arg(short, long)]
        target: String,

        /// Path to the hashtable file
        #[arg(short = 'H', long, visible_short_alias = 'd')]
        hashtable: Option<String>,

        /// Output the diffs to a .csv file
        #[arg(short, long, help = "The path to the output .csv file")]
        output: Option<String>,
    },
}

fn main() -> eyre::Result<()> {
    initialize_tracing()?;

    let args = Args::parse();

    match args.command {
        Commands::Extract {
            input,
            output,
            hashtable,
            filter_type,
            pattern,
        } => extract(ExtractArgs {
            input,
            output,
            hashtable,
            filter_type,
            pattern,
        }),
        Commands::Diff {
            reference,
            target,
            hashtable,
            output,
        } => diff(DiffArgs {
            reference,
            target,
            hashtable_path: hashtable,
            output,
        }),
    }
}

fn initialize_tracing() -> eyre::Result<()> {
    let indicatif_layer = IndicatifLayer::new();

    let common_format = fmt::format()
        .with_ansi(true)
        .with_level(true)
        .with_source_location(false)
        .with_line_number(false)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::time());

    // stdout: INFO/DEBUG/TRACE
    let stdout_layer = fmt::layer()
        .with_writer(indicatif_layer.get_stdout_writer())
        .event_format(common_format.clone())
        .with_filter(filter::filter_fn(|metadata| {
            let level = *metadata.level();
            level == Level::INFO || level == Level::DEBUG || level == Level::TRACE
        }));

    // stderr: WARN/ERROR
    let stderr_layer = fmt::layer()
        .with_writer(indicatif_layer.get_stderr_writer())
        .event_format(common_format)
        .with_filter(filter::filter_fn(|metadata| {
            let level = *metadata.level();
            level == Level::WARN || level == Level::ERROR
        }));

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(stderr_layer)
        .with(indicatif_layer)
        .with(LevelFilter::TRACE)
        .init();
    Ok(())
}

// parses filter type for clap arguments
fn parse_filter_type(s: &str) -> Result<LeagueFileKind, String> {
    match LeagueFileKind::from_extension(s) {
        LeagueFileKind::Unknown => Err(format!("Unknown file kind: {}", s)),
        other => Ok(other),
    }
}
