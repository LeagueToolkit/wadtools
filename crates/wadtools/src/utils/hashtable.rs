use color_eyre::eyre::{self, eyre, Result};
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
/// overlay for supplemental names - a user-provided `-H` text file or names
/// recovered at runtime. Lookups check the overlay first, then each base table.
///
/// The only League-domain policy layered on top of mimir is the **hex fallback**:
/// mimir returns `Option`, and a total miss is rendered as the 16-hex form of the
/// hash here (mimir never invents hex strings).
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
    pub fn resolve_path(&self, path_hash: u64) -> Cow<'_, str> {
        self.0
            .get(path_hash)
            .unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(path_hash)))
    }

    /// Resolves many chunk path hashes at once, in input order.
    #[allow(dead_code)]
    pub fn resolve_batch<'a>(&'a self, path_hashes: &'a [u64]) -> Vec<Cow<'a, str>> {
        self.0
            .get_batch(path_hashes)
            .map(|(hash, path)| path.unwrap_or_else(|| Cow::Owned(format_chunk_path_hash(hash))))
            .collect()
    }

    /// Returns true if any base table or the overlay knows this hash.
    pub fn contains(&self, path_hash: u64) -> bool {
        self.0.contains(path_hash)
    }

    /// Inserts a supplemental `hash -> path` mapping into the overlay.
    #[allow(dead_code)]
    pub fn insert(&mut self, path_hash: u64, path: impl Into<Box<str>>) {
        self.0.insert(path_hash, path);
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
