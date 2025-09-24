use std::{collections::HashMap, fs::File};

use league_toolkit::{
    file::LeagueFileKind,
    wad::{Wad, WadChunk},
};

use crate::{extractor::Extractor, utils::WadHashtable};
use fancy_regex::Regex;

pub struct ExtractArgs {
    pub input: String,
    pub output: String,
    pub hashtable: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
    pub pattern: Option<String>,
}

pub fn extract(args: ExtractArgs) -> eyre::Result<()> {
    let source = File::open(&args.input)?;

    let mut wad = Wad::mount(&source)?;

    let (mut decoder, chunks) = wad.decode();

    let mut hashtable = WadHashtable::new()?;
    if let Some(hashtable_path) = args.hashtable {
        tracing::info!("loading hashtable from {}", hashtable_path);
        hashtable.add_from_file(&File::open(&hashtable_path)?)?;
    }

    let mut extractor = Extractor::new(&mut decoder, &hashtable);

    let filter_pattern = create_filter_pattern(args.pattern)?;
    let extracted_count = get_extracted_count(chunks, &hashtable, filter_pattern.as_ref());

    extractor.set_filter_pattern(filter_pattern);
    extractor.extract_chunks(chunks, &args.output, args.filter_type.as_deref())?;

    tracing::info!("extracted {} chunks :)", extracted_count);

    Ok(())
}

fn create_filter_pattern(pattern: Option<String>) -> eyre::Result<Option<Regex>> {
    match pattern {
        Some(mut p) => {
            // Default to case-insensitive unless the user explicitly sets (?i) or (?-i)
            let has_inline_flag = p.contains("(?i)") || p.contains("(?-i)");
            if !has_inline_flag {
                p = format!("(?i){p}");
            }
            Ok(Some(Regex::new(&p)?))
        }
        None => Ok(None),
    }
}

fn get_extracted_count(
    chunks: &HashMap<u64, WadChunk>,
    hashtable: &WadHashtable,
    filter_pattern: Option<&Regex>,
) -> usize {
    match filter_pattern {
        Some(re) => chunks
            .values()
            .filter(|chunk| {
                re.is_match(hashtable.resolve_path(chunk.path_hash()).as_ref())
                    .unwrap_or(false)
            })
            .count(),
        None => chunks.len(),
    }
}
