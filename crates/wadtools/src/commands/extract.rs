use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::owo_colors::OwoColorize;
use std::fs::File;

use league_toolkit::{file::LeagueFileKind, wad::Wad};

use crate::{
    extractor::Extractor,
    utils::{create_filter_pattern, WadHashtable},
};
use convert_case::{Case, Casing};

pub struct ExtractArgs {
    pub input: String,
    pub output: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
    pub pattern: Option<String>,
    pub hash: Option<Vec<u64>>,
    pub filter_invert: bool,
    pub overwrite: bool,
}

pub fn extract(args: ExtractArgs, hashtable: &WadHashtable) -> eyre::Result<()> {
    let source = File::open(&args.input)?;

    let mut wad = Wad::mount(source)?;

    let mut extractor = Extractor::new(&mut wad, hashtable);

    let filter_pattern = create_filter_pattern(args.pattern)?;

    extractor.set_filter_pattern(filter_pattern);
    extractor.set_hash_filter(args.hash);
    extractor.set_filter_invert(args.filter_invert);
    let output_dir: Utf8PathBuf = match &args.output {
        Some(path) => Utf8PathBuf::from(path.as_str()),
        None => {
            // Construct sibling dir named after input file (without extension)
            let input_path = Utf8Path::new(&args.input);
            let parent = input_path.parent().unwrap_or(Utf8Path::new("."));
            let stem = input_path.file_stem().unwrap_or("extracted");
            parent.join(stem)
        }
    };
    let (extracted_count, skipped_existing) =
        extractor.extract_chunks(&output_dir, args.filter_type.as_deref(), args.overwrite)?;

    if skipped_existing > 0 {
        tracing::info!(
            "extracted {} chunks, skipped {} existing :)",
            extracted_count,
            skipped_existing
        );
    } else {
        tracing::info!("extracted {} chunks :)", extracted_count);
    }

    Ok(())
}

pub fn print_supported_filters() {
    println!("Supported filter types (name -> description [extension]):");
    for kind in LeagueFileKind::iter().collect::<Vec<_>>() {
        let ext = kind.extension().unwrap_or("");
        let snake = format!("{:?}", kind).to_case(Case::Snake);
        println!(
            "  {:24} -> {:?} [{}]",
            snake.bright_yellow().bold(),
            kind,
            ext.bright_green().bold()
        );
    }
}
