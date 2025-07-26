use std::fs::File;

use league_toolkit::core::wad::Wad;

use crate::{extractor::Extractor, league_file::LeagueFileKind, utils::WadHashtable};

pub struct ExtractArgs {
    pub input: String,
    pub output: String,
    pub hashtable: Option<String>,
    pub filter_type: Option<Vec<LeagueFileKind>>,
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

    let mut extractor = Extractor::new(&mut decoder, &hashtable);
    let filter_type = args.filter_type.as_ref().map(|x| x.as_slice());
    extractor.extract_chunks(&chunks, &args.output, filter_type)?;

    tracing::info!("extracted {} chunks :)", chunks.len());

    Ok(())
}
