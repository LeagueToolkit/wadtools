use camino::Utf8Path;
use color_eyre::eyre::{self, eyre, Result};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    sync::Arc,
};
use tracing::info;
use walkdir::WalkDir;

use super::format_chunk_path_hash;

#[derive(Debug, Clone, Default)]
pub struct WadHashtable {
    items: HashMap<u64, Arc<str>>,
}

impl WadHashtable {
    pub fn new() -> Result<Self> {
        Ok(WadHashtable {
            items: HashMap::default(),
        })
    }

    pub fn resolve_path(&self, path_hash: u64) -> Arc<str> {
        self.items
            .get(&path_hash)
            .cloned()
            .unwrap_or_else(|| format_chunk_path_hash(path_hash).into())
    }

    #[allow(dead_code)]
    pub fn add_from_dir(&mut self, dir: impl AsRef<Utf8Path>) -> eyre::Result<()> {
        for wad_hashtable_entry in WalkDir::new(dir.as_ref().as_std_path())
            .into_iter()
            .filter_map(|x| x.ok())
        {
            if !wad_hashtable_entry.file_type().is_file() {
                continue;
            }

            info!("loading wad hasthable: {:?}", wad_hashtable_entry.path());
            self.add_from_file(&File::open(wad_hashtable_entry.path())?)?;
        }

        Ok(())
    }

    pub fn add_from_file(&mut self, file: &File) -> eyre::Result<()> {
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(Ok(line)) = lines.next() {
            let mut components = line.split(' ');

            let hash = components.next().ok_or(eyre!("failed to read hash"))?;
            let hash = u64::from_str_radix(hash, 16).expect("failed to convert hash");
            let path = itertools::join(components, " ");

            self.items.insert(hash, path.into());
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn items(&self) -> &HashMap<u64, Arc<str>> {
        &self.items
    }
    #[allow(dead_code)]
    pub fn items_mut(&mut self) -> &mut HashMap<u64, Arc<str>> {
        &mut self.items
    }
}
