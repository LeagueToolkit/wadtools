use crate::utils::{is_chunk_path, WadHashtable};
use color_eyre::eyre::{self, Ok};
use eyre::Context;
use fancy_regex::Regex;
use league_toolkit::{
    file::LeagueFileKind,
    wad::{WadChunk, WadDecoder},
};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, DirBuilder, File},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::style::ProgressStyle;

pub struct Extractor<'chunks> {
    decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
    hashtable: &'chunks WadHashtable,
    filter_pattern: Option<Regex>,
}

impl<'chunks> Extractor<'chunks> {
    pub fn new(
        decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
        hashtable: &'chunks WadHashtable,
    ) -> Self {
        Self {
            decoder,
            hashtable,
            filter_pattern: None,
        }
    }

    pub fn set_filter_pattern(&mut self, filter_pattern: Option<Regex>) {
        self.filter_pattern = filter_pattern;
    }

    pub fn extract_chunks(
        &mut self,
        chunks: &HashMap<u64, WadChunk>,
        extract_directory: impl AsRef<Path>,
        filter_type: Option<&[LeagueFileKind]>,
    ) -> eyre::Result<()> {
        let total = chunks.len() as u64;
        let span = tracing::info_span!("extract", total = total);
        let _entered = span.enter();
        span.pb_set_style(
            &ProgressStyle::with_template("{wide_bar:40.cyan/blue} {pos}/{len} \n {spinner} {msg}")
                .unwrap(),
        );
        span.pb_set_length(total);
        span.pb_set_message("Extracting chunks");
        span.pb_set_finish_message("Extraction complete");

        prepare_extraction_directories_absolute(
            chunks.iter(),
            self.hashtable,
            &extract_directory,
            self.filter_pattern.as_ref(),
        )?;

        extract_wad_chunks(
            self.decoder,
            chunks,
            self.hashtable,
            extract_directory.as_ref().to_path_buf(),
            |progress, message| {
                // progress is 0.0..1.0; convert to absolute position
                let position = (progress * total as f64).round() as u64;
                span.pb_set_position(position);
                if let Some(msg) = message {
                    span.pb_set_message(msg);
                }
                Ok(())
            },
            filter_type,
            self.filter_pattern.as_ref(),
        )?;

        Ok(())
    }
}

pub fn prepare_extraction_directories_absolute<'chunks>(
    chunks: impl Iterator<Item = (&'chunks u64, &'chunks WadChunk)>,
    wad_hashtable: &WadHashtable,
    extraction_directory: impl AsRef<Path>,
    filter_pattern: Option<&Regex>,
) -> eyre::Result<()> {
    // collect all chunk directories
    let chunk_directories = chunks.filter_map(|(_, chunk)| {
        let chunk_path_str = wad_hashtable.resolve_path(chunk.path_hash());
        if let Some(regex) = filter_pattern {
            if !regex.is_match(chunk_path_str.as_ref()).unwrap_or(false) {
                return None;
            }
        }
        Path::new(chunk_path_str.as_ref())
            .parent()
            .map(|path| path.to_path_buf())
    });

    create_extraction_directories(chunk_directories, extraction_directory)?;

    Ok(())
}

fn create_extraction_directories(
    chunk_directories: impl Iterator<Item = impl AsRef<Path>>,
    extraction_directory: impl AsRef<Path>,
) -> eyre::Result<()> {
    // this wont error if the directory already exists since recursive mode is enabled
    for chunk_directory in chunk_directories {
        DirBuilder::new()
            .recursive(true)
            .create(extraction_directory.as_ref().join(chunk_directory))?;
    }

    Ok(())
}

pub fn extract_wad_chunks<TSource: Read + Seek>(
    decoder: &mut WadDecoder<TSource>,
    chunks: &HashMap<u64, WadChunk>,
    wad_hashtable: &WadHashtable,
    extract_directory: PathBuf,
    report_progress: impl Fn(f64, Option<&str>) -> eyre::Result<()>,
    filter_type: Option<&[LeagueFileKind]>,
    filter_pattern: Option<&Regex>,
) -> eyre::Result<()> {
    let mut i = 0;
    for chunk in chunks.values() {
        let chunk_path_str = wad_hashtable.resolve_path(chunk.path_hash());
        let chunk_path = Path::new(chunk_path_str.as_ref());

        // advance progress for every chunk (including ones we skip)
        report_progress(i as f64 / chunks.len() as f64, chunk_path.to_str())?;

        if let Some(regex) = filter_pattern {
            if !regex.is_match(chunk_path_str.as_ref()).unwrap_or(false) {
                i += 1;
                continue;
            }
        }

        extract_wad_chunk_absolute(decoder, chunk, chunk_path, &extract_directory, filter_type)?;

        i += 1;
    }

    Ok(())
}

pub fn extract_wad_chunk_absolute<'wad, TSource: Read + Seek>(
    decoder: &mut WadDecoder<'wad, TSource>,
    chunk: &WadChunk,
    chunk_path: impl AsRef<Path>,
    extract_directory: impl AsRef<Path>,
    filter_type: Option<&[LeagueFileKind]>,
) -> eyre::Result<()> {
    let chunk_data = decoder.load_chunk_decompressed(chunk).wrap_err(format!(
        "failed to decompress chunk (chunk_path: {})",
        chunk_path.as_ref().display()
    ))?;

    let chunk_kind = LeagueFileKind::identify_from_bytes(&chunk_data);
    if filter_type.is_some_and(|filter| !filter.contains(&chunk_kind)) {
        tracing::debug!(
            "skipping chunk (chunk_path: {}, chunk_kind: {:?})",
            chunk_path.as_ref().display(),
            chunk_kind
        );
        return Ok(());
    }

    let chunk_path = resolve_final_chunk_path(chunk_path, &chunk_data);
    let Err(error) = fs::write(extract_directory.as_ref().join(&chunk_path), &chunk_data) else {
        return Ok(());
    };

    // This will happen if the filename is too long
    if error.kind() == io::ErrorKind::InvalidFilename {
        write_long_filename_chunk(chunk, chunk_path, extract_directory, &chunk_data)
    } else {
        Err(error).wrap_err(format!(
            "failed to write chunk (chunk_path: {})",
            chunk_path.display()
        ))
    }
}

fn resolve_final_chunk_path(chunk_path: impl AsRef<Path>, chunk_data: &[u8]) -> PathBuf {
    let mut chunk_path = chunk_path.as_ref().to_path_buf();
    if chunk_path.extension().is_none() && is_chunk_path(&chunk_path) {
        // check for known extensions
        match LeagueFileKind::identify_from_bytes(chunk_data) {
            LeagueFileKind::Unknown => {
                tracing::warn!(
                    "chunk has no known extension, prepending '.' (chunk_path: {})",
                    chunk_path.display()
                );

                chunk_path = chunk_path.with_file_name(OsStr::new(
                    &(".".to_string() + chunk_path.file_name().unwrap().to_string_lossy().as_ref()),
                ));
            }
            file_kind => {
                chunk_path.set_extension(file_kind.extension().unwrap());
            }
        }
    }

    chunk_path
}

fn write_long_filename_chunk(
    chunk: &WadChunk,
    chunk_path: impl AsRef<Path>,
    extract_directory: impl AsRef<Path>,
    chunk_data: &[u8],
) -> eyre::Result<()> {
    let hashed_path = format!("{:016x}", chunk.path_hash());
    tracing::warn!(
        "invalid chunk filename, writing as hashed path (chunk_path: {}, hashed_path: {})",
        chunk_path.as_ref().display(),
        &hashed_path
    );

    let file_kind = LeagueFileKind::identify_from_bytes(chunk_data);
    let extension = file_kind.extension();

    match file_kind {
        LeagueFileKind::Unknown => {
            fs::write(extract_directory.as_ref().join(hashed_path), chunk_data)?;
        }
        _ => {
            fs::write(
                extract_directory
                    .as_ref()
                    .join(format!("{:016x}", chunk.path_hash()))
                    .with_extension(extension.unwrap()),
                chunk_data,
            )?;
        }
    }

    Ok(())
}
