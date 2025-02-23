use std::fs::File;

use clap::Args;
use league_toolkit::core::wad::Wad;
use regex::Regex;

use crate::{extractor::Extractor, utils::WadHashtable};

#[derive(Args, Debug, Clone)]
pub struct ExtractArgs {
    /// Path to the input wad file
    pub input: String,
    /// Path to the output directory
    pub output: String,
    /// Path to the hashtable file
    #[arg(long)]
    pub hashtable: Option<String>,
    #[arg(short, long)]
    pub filter: Option<String>,
}

pub fn extract(args: ExtractArgs) -> eyre::Result<()> {
    let source = File::open(&args.input)?;
    let mut wad = Wad::mount(&source)?;

    let (mut decoder, chunks) = wad.decode();

    let mut hashtable = WadHashtable::new()?;
    if let Some(hashtable_path) = args.hashtable {
        tracing::info!("loading hashtable from {}", hashtable_path);
        hashtable.add_from_file(&mut File::open(&hashtable_path)?)?;
    }

    let filter = args.filter.map_or(Ok(None), |v| Regex::new(&v).map(Some))?;

    let mut extractor = Extractor::new(&mut decoder, &hashtable, filter);
    extractor.extract_chunks(&chunks, &args.output)?;

    tracing::info!("extracted {} chunks :)", chunks.len());

    Ok(())
}
