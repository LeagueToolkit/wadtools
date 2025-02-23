#![feature(io_error_more)]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(let_chains)]

use std::io::stdout;

use clap::{Parser, Subcommand};
use tracing::Level;
mod commands;
mod extractor;
mod league_file;
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
    Extract(ExtractArgs),
    /// Compare two wad files
    ///
    /// This command compares two wad files and prints the differences between them.
    /// Using the reference wad file, it will print the differences between the target wad file.
    ///
    Diff {
        /// Path to the reference wad file
        #[arg(short, long)]
        reference: String,

        /// Path to the target wad file
        #[arg(short, long)]
        target: String,

        /// Path to the hashtable file
        #[arg(short, long)]
        hashtable_path: Option<String>,

        /// Output the diffs to a .csv file
        #[arg(short, long, help = "The path to the output .csv file")]
        output: Option<String>,
    },
}

fn main() -> eyre::Result<()> {
    initialize_tracing()?;

    let args = Args::parse();

    match args.command {
        Commands::Extract(args) => extract(args),
        Commands::Diff {
            reference,
            target,
            hashtable_path,
            output,
        } => diff(DiffArgs {
            reference,
            target,
            hashtable_path,
            output,
        }),
    }
}

fn initialize_tracing() -> eyre::Result<()> {
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(stdout)
        .event_format(
            tracing_subscriber::fmt::format()
                .with_ansi(true)
                .with_level(true)
                .with_source_location(false)
                .with_line_number(false)
                .with_target(false)
                .with_timer(tracing_subscriber::fmt::time::time()),
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}
