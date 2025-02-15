#![feature(io_error_more)]

use std::io::stdout;

use clap::{Parser, Subcommand};
use tracing::Level;
mod commands;
mod extractor;
mod league_file;
mod utils;

use commands::*;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Extract {
        /// Path to the input wad file
        #[arg(short, long)]
        input: String,

        /// Path to the output directory
        #[arg(short, long)]
        output: String,

        /// Path to the hashtable file
        #[arg(short, long)]
        hashtable: Option<String>,
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
        } => extract(ExtractArgs {
            input,
            output,
            hashtable,
        }),
    }
}

fn initialize_tracing() -> eyre::Result<()> {
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_max_level(Level::INFO)
        .with_writer(stdout)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}
