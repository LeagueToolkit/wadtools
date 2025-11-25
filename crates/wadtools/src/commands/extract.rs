use camino::{Utf8Path, Utf8PathBuf};
use color_eyre::owo_colors::OwoColorize;
use std::fs::File;

use league_toolkit::{file::LeagueFileKind, wad::Wad};

use crate::{
    extractor::Extractor,
    utils::{create_filter_pattern, default_hashtable_dir, WadHashtable},
};
use convert_case::{Case, Casing};

pub struct ExtractArgs {
    pub input: String,
    pub output: Option<String>,
    pub hashtable: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
    pub pattern: Option<String>,
    pub hashtable_dir: Option<String>,
}

pub fn extract(args: ExtractArgs) -> eyre::Result<()> {
    let source = File::open(&args.input)?;

    let mut wad = Wad::mount(&source)?;

    let (mut decoder, chunks) = wad.decode();

    let mut hashtable = WadHashtable::new()?;
    if let Some(dir_override) = &args.hashtable_dir {
        hashtable.add_from_dir(Utf8Path::new(dir_override))?;
    } else if let Some(dir) = default_hashtable_dir() {
        hashtable.add_from_dir(dir)?;
    }
    if let Some(hashtable_path) = args.hashtable {
        tracing::info!("loading hashtable from {}", hashtable_path);
        hashtable.add_from_file(&File::open(&hashtable_path)?)?;
    }

    let mut extractor = Extractor::new(&mut decoder, &hashtable);

    let filter_pattern = create_filter_pattern(args.pattern)?;

    extractor.set_filter_pattern(filter_pattern);
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
    let extracted_count =
        extractor.extract_chunks(chunks, &output_dir, args.filter_type.as_deref())?;

    tracing::info!("extracted {} chunks :)", extracted_count);

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
