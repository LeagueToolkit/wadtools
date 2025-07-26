use crate::{
    league_file::{get_extension_from_league_file_kind, identify_league_file, LeagueFileKind},
    utils::{is_chunk_path, WadHashtable},
};
use color_eyre::eyre::{self, Ok};
use eyre::Context;
use league_toolkit::core::wad::{WadChunk, WadDecoder};
use std::{
    collections::HashMap,
    ffi::OsStr,
    fs::{self, DirBuilder, File},
    io::{self, Read, Seek},
    path::{Path, PathBuf},
};

pub struct Extractor<'chunks> {
    decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
    hashtable: &'chunks WadHashtable,
}

impl<'chunks> Extractor<'chunks> {
    pub fn new(
        decoder: &'chunks mut WadDecoder<'chunks, &'chunks File>,
        hashtable: &'chunks WadHashtable,
    ) -> Self {
        Self { decoder, hashtable }
    }

    pub fn extract_chunks(
        &mut self,
        chunks: &HashMap<u64, WadChunk>,
        extract_directory: impl AsRef<Path>,
        filter_type: Option<&[LeagueFileKind]>,
    ) -> eyre::Result<()> {
        tracing::info!("preparing extraction directories");
        prepare_extraction_directories_absolute(chunks.iter(), self.hashtable, &extract_directory)?;

        tracing::info!("extracting chunks");
        extract_wad_chunks(
            self.decoder,
            chunks,
            self.hashtable,
            extract_directory.as_ref().to_path_buf(),
            |_, _| Ok(()),
            filter_type,
        )?;

        Ok(())
    }
}

pub fn prepare_extraction_directories_absolute<'chunks>(
    chunks: impl Iterator<Item = (&'chunks u64, &'chunks WadChunk)>,
    wad_hashtable: &WadHashtable,
    extraction_directory: impl AsRef<Path>,
) -> eyre::Result<()> {
    tracing::info!("preparing absolute extraction directories");

    // collect all chunk directories
    let chunk_directories = chunks.filter_map(|(_, chunk)| {
        Path::new(wad_hashtable.resolve_path(chunk.path_hash()).as_ref())
            .parent()
            .map(|path| path.to_path_buf())
    });

    create_extraction_directories(chunk_directories, extraction_directory)?;

    Ok(())
}

pub fn prepare_extraction_directories_relative<'chunks>(
    chunks: impl Iterator<Item = &'chunks WadChunk>,
    parent_path: Option<impl AsRef<Path>>,
    wad_hashtable: &WadHashtable,
    extraction_directory: impl AsRef<Path>,
) -> eyre::Result<()> {
    tracing::info!("preparing relative extraction directories");

    // collect all chunk directories
    let chunk_directories = chunks.filter_map(|chunk| {
        let chunk_directory = wad_hashtable.resolve_path(chunk.path_hash());
        let chunk_directory = Path::new(chunk_directory.as_ref()).parent().unwrap();

        match &parent_path {
            Some(parent_path) => chunk_directory
                .strip_prefix(parent_path.as_ref())
                .ok()
                .map(|path| path.to_path_buf()),
            None => Some(chunk_directory.to_path_buf()),
        }
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
) -> eyre::Result<()> {
    tracing::info!("extracting chunks");

    let mut i = 0;
    for (_, chunk) in chunks {
        let chunk_path = wad_hashtable.resolve_path(chunk.path_hash());
        let chunk_path = Path::new(chunk_path.as_ref());

        report_progress(i as f64 / chunks.len() as f64, chunk_path.to_str())?;

        extract_wad_chunk_absolute(
            decoder,
            &chunk,
            &chunk_path,
            &extract_directory,
            filter_type,
        )?;

        i = i + 1;
    }

    Ok(())
}

pub fn extract_wad_chunks_relative<TSource: Read + Seek>(
    decoder: &mut WadDecoder<TSource>,
    chunks: &Vec<WadChunk>,
    base_directory: Option<impl AsRef<Path>>,
    wad_hashtable: &WadHashtable,
    extract_directory: PathBuf,
    report_progress: impl Fn(f64, Option<&str>) -> eyre::Result<()>,
    filter_type: Option<&[LeagueFileKind]>,
) -> eyre::Result<()> {
    tracing::info!("extracting chunks");

    let mut i = 0;
    for chunk in chunks {
        let chunk_path = wad_hashtable.resolve_path(chunk.path_hash());
        let chunk_path = Path::new(chunk_path.as_ref());

        report_progress(i as f64 / chunks.len() as f64, chunk_path.to_str())?;

        extract_wad_chunk_absolute(
            decoder,
            &chunk,
            match base_directory {
                Some(ref base_directory) => &chunk_path.strip_prefix(base_directory.as_ref())?,
                None => chunk_path,
            },
            &extract_directory,
            filter_type
        )?;

        i = i + 1;
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

    let chunk_kind = identify_league_file(&chunk_data);
    if filter_type.is_some_and(|filter| !filter.contains(&chunk_kind)) {
        tracing::debug!(
            "skipping chunk (chunk_path: {}, chunk_kind: {:?})",
            chunk_path.as_ref().display(),
            chunk_kind
        );
        return Ok(());
    }

    let chunk_path = resolve_final_chunk_path(chunk_path, &chunk_data);
    let Err(error) = fs::write(&extract_directory.as_ref().join(&chunk_path), &chunk_data) else {
        return Ok(());
    };

    // This will happen if the filename is too long
    if error.kind() == io::ErrorKind::InvalidFilename {
        write_long_filename_chunk(chunk, chunk_path, extract_directory, &chunk_data)
    } else {
        return Err(error).wrap_err(format!(
            "failed to write chunk (chunk_path: {})",
            chunk_path.display()
        ));
    }
}

fn resolve_final_chunk_path(chunk_path: impl AsRef<Path>, chunk_data: &Box<[u8]>) -> PathBuf {
    let mut chunk_path = chunk_path.as_ref().to_path_buf();
    if chunk_path.extension().is_none() && is_chunk_path(&chunk_path) {
        // check for known extensions
        match identify_league_file(&chunk_data) {
            LeagueFileKind::Unknown => {
                tracing::warn!(
                    "chunk has no known extension, prepending '.' (chunk_path: {})",
                    chunk_path.display()
                );

                chunk_path = chunk_path.with_file_name(OsStr::new(
                    &(".".to_string()
                        + &chunk_path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .to_string()),
                ));
            }
            file_kind => {
                let extension = get_extension_from_league_file_kind(file_kind);
                chunk_path.set_extension(extension);
            }
        }
    }

    chunk_path
}

fn write_long_filename_chunk(
    chunk: &WadChunk,
    chunk_path: impl AsRef<Path>,
    extract_directory: impl AsRef<Path>,
    chunk_data: &Box<[u8]>,
) -> eyre::Result<()> {
    let hashed_path = format!("{:016x}", chunk.path_hash());
    tracing::warn!(
        "invalid chunk filename, writing as hashed path (chunk_path: {}, hashed_path: {})",
        chunk_path.as_ref().display(),
        &hashed_path
    );

    let file_kind = identify_league_file(&chunk_data);
    let extension = get_extension_from_league_file_kind(file_kind);

    match file_kind {
        LeagueFileKind::Unknown => {
            fs::write(&extract_directory.as_ref().join(hashed_path), &chunk_data)?;
        }
        _ => {
            fs::write(
                &extract_directory
                    .as_ref()
                    .join(format!("{:016x}", chunk.path_hash()))
                    .with_extension(extension),
                &chunk_data,
            )?;
        }
    }

    Ok(())
}
