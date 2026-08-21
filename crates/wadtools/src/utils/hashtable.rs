use color_eyre::eyre::{self, eyre, Result};
use league_toolkit::wad::{PathResolver, WadHash};
use ltk_hashdb::LayeredHashDb;
use ltk_mimir_cache::{HashStore, Table};
use std::{
    borrow::Cow,
    fs::File,
    io::{BufRead, BufReader},
};
use tracing::{info, warn};

use super::format_chunk_path_hash;

/// Resolves WAD chunk path hashes to readable paths.
///
/// A thin wrapper over mimir's [`LayeredHashDb`]: the Game and Lcu `.lhdb` base
/// tables (opened lazily via `mmap` from the shared cache) sit under an in-memory
/// overlay for supplemental names from a user-provided `-H` text file. Lookups
/// check the overlay first, then each base table.
///
/// The only League-domain policy layered on top of mimir is the **hex fallback**:
/// mimir returns `Option`, and a total miss is rendered as the 16-hex form of the
/// hash here (mimir never invents hex strings). As a [`PathResolver`] the table
/// answers `None` instead, and `ltk_wad` applies the same fallback itself.
#[derive(Default)]
pub struct WadHashtable(LayeredHashDb);

impl WadHashtable {
    /// Creates an empty hashtable (no base tables, empty overlay). Used by tests
    /// and callers that only need the overlay.
    #[allow(dead_code)]
    pub fn new() -> Result<Self> {
        Ok(WadHashtable::default())
    }

    /// Opens the Game and Lcu tables from the given mimir cache store into one
    /// layered reader.
    ///
    /// A table that cannot be opened (cache never populated, missing file) is
    /// warned about and skipped, so the tool still runs - unknown hashes just
    /// fall back to their hex representation until `download-hashes` is run.
    pub fn from_store(store: &HashStore) -> Self {
        let (layered, errors) = store.open_layered(&[Table::Game, Table::Lcu]);
        for (table, error) in &errors {
            warn!(
                "could not open hash table '{}': {error}. \
                 Run `wadtools download-hashes` to populate the cache.",
                table.id()
            );
        }
        for base in layered.bases() {
            info!("loaded hash table ({} entries)", base.len());
        }

        WadHashtable(layered)
    }

    /// Resolves a chunk path hash to a readable path, falling back to the 16-hex
    /// representation when unknown.
    pub fn resolve_path(&self, path_hash: WadHash) -> Cow<'_, str> {
        self.0
            .get(path_hash.0)
            .unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(path_hash)))
    }

    /// Resolves many chunk path hashes at once, in input order.
    ///
    /// Takes the raw hashes, as mimir's bulk lookup does.
    pub fn resolve_batch<'a>(&'a self, path_hashes: &'a [u64]) -> Vec<Cow<'a, str>> {
        self.0
            .get_batch(path_hashes)
            .map(|(hash, path)| {
                path.unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(WadHash(hash))))
            })
            .collect()
    }

    /// Inserts a supplemental `hash -> path` mapping into the overlay.
    #[allow(dead_code)]
    pub fn insert(&mut self, path_hash: WadHash, path: impl Into<Box<str>>) {
        self.0.insert(path_hash.0, path);
    }

    /// Loads supplemental `<hex-hash> <path>` lines from a text file into the
    /// overlay. Kept for the `-H/--hashtable` flag so users can supply their own
    /// path lists on top of the shared tables.
    pub fn add_from_file(&mut self, file: &File) -> eyre::Result<()> {
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        while let Some(Ok(line)) = lines.next() {
            let mut components = line.split(' ');

            let hash = components.next().ok_or(eyre!("failed to read hash"))?;
            let hash = u64::from_str_radix(hash, 16).expect("failed to convert hash");
            let path = itertools::join(components, " ");

            self.0.insert(hash, path);
        }

        Ok(())
    }
}

impl PathResolver for WadHashtable {
    fn resolve(&self, path_hash: WadHash) -> Option<Cow<'_, str>> {
        self.0.get(path_hash.0)
    }

    fn is_known(&self, path_hash: WadHash) -> bool {
        self.0.contains(path_hash.0)
    }
}
