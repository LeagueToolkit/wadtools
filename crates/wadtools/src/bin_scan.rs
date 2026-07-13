//! Pre-extraction scan of `.bin` property files to recover chunk names.
//!
//! WAD chunks are keyed only by `xxh64(lowercase_path)`. Chunks whose hash is not in
//! the loaded community hashtable would otherwise extract as anonymous 16-hex names.
//! League `.bin` files reference other assets by path - both as explicit dependency
//! links (`BinTree::dependencies`) and as string properties (textures, meshes, vfx,
//! ...). Hashing those discovered paths and matching them against the WAD's own chunk
//! hashes lets us fill in real names/folders before extraction.
//!
//! Matching is exact: a discovered string only contributes a name when its hash equals
//! an actual unresolved chunk hash, so non-path strings simply never match.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{Cursor, Read, Seek},
    sync::Arc,
};

use camino::Utf8Path;
use league_toolkit::{
    file::LeagueFileKind,
    meta::{BinTree, PropertyValueEnum},
    wad::{decompress_raw, Wad, WadChunk},
};
use xxhash_rust::xxh64::xxh64;

use crate::utils::WadHashtable;

/// Computes the WAD path hash for a string, matching how chunk hashes are produced
/// (`xxh64` of the lowercased path, seed 0).
pub fn hash_wad_path(path: &str) -> u64 {
    xxh64(path.to_lowercase().as_bytes(), 0)
}

/// Parses a decompressed `.bin` file and appends every path-like string it contains
/// (dependency links plus all string property values) to `out`.
///
/// Parse failures are non-fatal: a malformed bin is logged at debug level and skipped.
pub fn collect_bin_paths(bin_bytes: &[u8], out: &mut Vec<String>) {
    let tree = match BinTree::from_reader(&mut Cursor::new(bin_bytes)) {
        Ok(tree) => tree,
        Err(error) => {
            tracing::debug!("failed to parse bin during scan: {error}");
            return;
        }
    };

    for dependency in &tree.dependencies {
        out.push(dependency.clone());
    }

    for (_path_hash, object) in &tree.objects {
        for property in object.properties.values() {
            walk_value(&property.value, out);
        }
    }
}

/// Recursively collects string values from a property value, descending into every
/// container/struct/map variant.
fn walk_value(value: &PropertyValueEnum, out: &mut Vec<String>) {
    match value {
        PropertyValueEnum::String(string) => {
            if !string.0.is_empty() {
                out.push(string.0.clone());
            }
        }
        PropertyValueEnum::Container(container) => {
            for item in &container.items {
                walk_value(item, out);
            }
        }
        PropertyValueEnum::UnorderedContainer(container) => {
            for item in &container.0.items {
                walk_value(item, out);
            }
        }
        PropertyValueEnum::Struct(value) => {
            for property in value.properties.values() {
                walk_value(&property.value, out);
            }
        }
        PropertyValueEnum::Embedded(value) => {
            for property in value.0.properties.values() {
                walk_value(&property.value, out);
            }
        }
        PropertyValueEnum::Optional(optional) => {
            if let Some(inner) = &optional.value {
                walk_value(inner, out);
            }
        }
        PropertyValueEnum::Map(map) => {
            for (key, value) in &map.entries {
                walk_value(&key.0, out);
                walk_value(value, out);
            }
        }
        // Hash / WadChunkLink / ObjectLink hold hashes that are not reversible to a
        // string path, and the remaining variants are scalars. Nothing to collect.
        _ => {}
    }
}

/// Returns true if `path` has a `.bin` extension (case-insensitive).
fn is_bin_path(path: &str) -> bool {
    Utf8Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bin"))
}

/// Loads and decompresses a chunk, returning `None` (with a debug log) on failure.
fn load_and_decompress<S: Read + Seek>(wad: &mut Wad<S>, chunk: &WadChunk) -> Option<Box<[u8]>> {
    let raw = match wad.load_chunk_raw(chunk) {
        Ok(raw) => raw,
        Err(error) => {
            tracing::debug!("failed to read chunk {:016x}: {error}", chunk.path_hash);
            return None;
        }
    };
    match decompress_raw(&raw, chunk.compression_type, chunk.uncompressed_size) {
        Ok(data) => Some(data),
        Err(error) => {
            tracing::debug!(
                "failed to decompress chunk {:016x}: {error}",
                chunk.path_hash
            );
            None
        }
    }
}

/// Parses one decompressed bin and folds its discovered names into `discovered`. A name
/// is kept only when its hash matches a real chunk in `chunk_hashes` and is not already
/// known - the overlay is purely additive gap-fill. Returns the chunk hashes of any
/// newly discovered paths that are themselves `.bin` files, so the caller can follow
/// links transitively.
fn harvest_bin(
    data: &[u8],
    chunk_hashes: &HashSet<u64>,
    hashtable: &WadHashtable,
    discovered: &mut HashMap<u64, Arc<str>>,
    scratch: &mut Vec<String>,
) -> Vec<u64> {
    scratch.clear();
    collect_bin_paths(data, scratch);

    let mut new_bins = Vec::new();
    for string in scratch.drain(..) {
        let hash = hash_wad_path(&string);
        if !chunk_hashes.contains(&hash)
            || discovered.contains_key(&hash)
            || hashtable.contains(hash)
        {
            continue;
        }
        let is_bin = is_bin_path(&string);
        discovered.insert(hash, Arc::from(string));
        if is_bin {
            new_bins.push(hash);
        }
    }
    new_bins
}

/// Scans the `.bin` files of a WAD and returns an overlay of `path_hash -> path` for
/// paths discovered inside them (dependency links plus string properties). A discovered
/// path is kept only when its hash matches an actual chunk in `chunk_hashes` and the
/// community `hashtable` does not already resolve it - the overlay is purely additive
/// gap-fill that never overrides official names.
///
/// Two modes:
/// - default (`full == false`): start from chunks the `hashtable` already names `*.bin`,
///   then follow discovered bin links transitively until no new bins are found. Only bins
///   we have a name for are decompressed, so the cost is a small slice of the WAD.
/// - full (`full == true`): decompress every chunk and magic-detect bins, so even bins
///   that are anonymous *and* unreferenced get harvested. Recovers the most names, at the
///   cost of a full extra decompression pass over the WAD.
pub fn scan_wad_bin_paths<S: Read + Seek>(
    wad: &mut Wad<S>,
    hashtable: &WadHashtable,
    chunk_hashes: &HashSet<u64>,
    full: bool,
) -> HashMap<u64, Arc<str>> {
    let mut discovered: HashMap<u64, Arc<str>> = HashMap::new();
    let mut scratch: Vec<String> = Vec::new();
    let chunks: Vec<WadChunk> = wad.chunks().iter().copied().collect();

    if full {
        for chunk in &chunks {
            let Some(data) = load_and_decompress(wad, chunk) else {
                continue;
            };
            if !matches!(
                LeagueFileKind::identify_from_bytes(&data),
                LeagueFileKind::PropertyBin | LeagueFileKind::PropertyBinOverride
            ) {
                continue;
            }
            harvest_bin(
                &data,
                chunk_hashes,
                hashtable,
                &mut discovered,
                &mut scratch,
            );
        }
        return discovered;
    }

    // Seed the queue with chunks already named `*.bin`, then follow discovered bin links.
    let chunk_by_hash: HashMap<u64, WadChunk> = chunks
        .iter()
        .map(|chunk| (chunk.path_hash, *chunk))
        .collect();
    let mut queue: VecDeque<u64> = chunks
        .iter()
        .filter(|chunk| is_bin_path(&hashtable.resolve_path(chunk.path_hash)))
        .map(|chunk| chunk.path_hash)
        .collect();
    let mut parsed: HashSet<u64> = HashSet::new();

    while let Some(hash) = queue.pop_front() {
        if !parsed.insert(hash) {
            continue;
        }
        let Some(&chunk) = chunk_by_hash.get(&hash) else {
            continue;
        };
        let Some(data) = load_and_decompress(wad, &chunk) else {
            continue;
        };
        let new_bins = harvest_bin(
            &data,
            chunk_hashes,
            hashtable,
            &mut discovered,
            &mut scratch,
        );
        queue.extend(new_bins);
    }

    discovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use league_toolkit::meta::value::{ContainerValue, StringValue};
    use league_toolkit::meta::{BinPropertyKind, BinTree, BinTreeObject};

    #[test]
    fn hash_wad_path_is_lowercased_xxh64() {
        assert_eq!(hash_wad_path("ASSETS/X.dds"), xxh64(b"assets/x.dds", 0));
        assert_eq!(hash_wad_path("assets/x.dds"), xxh64(b"assets/x.dds", 0));
    }

    #[test]
    fn collect_bin_paths_gathers_dependencies_and_nested_strings() {
        // A nested container holding a string, plus a top-level string property and a
        // dependency link.
        let container = ContainerValue {
            item_kind: BinPropertyKind::String,
            items: vec![StringValue("ASSETS/Characters/Foo/Foo.dds".into()).into()],
        };
        let object = BinTreeObject::builder(0x1, 0x2)
            .property(0xAA, StringValue("ASSETS/Top.bin".into()))
            .property(0xBB, container)
            .build();
        let tree = BinTree::builder()
            .dependency("DATA/base.bin")
            .object(object)
            .build();

        let mut buffer = Cursor::new(Vec::new());
        tree.to_writer(&mut buffer).unwrap();
        let bytes = buffer.into_inner();

        let mut out = Vec::new();
        collect_bin_paths(&bytes, &mut out);

        assert!(out.contains(&"DATA/base.bin".to_string()));
        assert!(out.contains(&"ASSETS/Top.bin".to_string()));
        assert!(out.contains(&"ASSETS/Characters/Foo/Foo.dds".to_string()));
    }

    #[test]
    fn collect_bin_paths_ignores_garbage() {
        let mut out = Vec::new();
        collect_bin_paths(&[0, 1, 2, 3, 4, 5, 6, 7], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn scan_wad_recovers_referenced_asset_name() {
        use league_toolkit::wad::{Wad, WadBuilder, WadChunkBuilder};
        use std::io::Write;

        // A bin (with a known name) references an asset whose hash is otherwise unknown.
        let asset_path = "assets/characters/foo/recovered.dds";
        let tree = BinTree::builder().dependency(asset_path).build();
        let mut bin_buffer = Cursor::new(Vec::new());
        tree.to_writer(&mut bin_buffer).unwrap();
        let bin_bytes = bin_buffer.into_inner();

        let bin_hash = hash_wad_path("data/test.bin");
        let asset_hash = hash_wad_path(asset_path);

        // Build an in-memory WAD with the bin chunk and the referenced (unnamed) asset.
        let mut wad_buffer = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(WadChunkBuilder::default().with_path("data/test.bin"))
            .with_chunk(WadChunkBuilder::default().with_path(asset_path))
            .build_to_writer(&mut wad_buffer, |path_hash, cursor| {
                if path_hash == bin_hash {
                    cursor.write_all(&bin_bytes)?;
                } else {
                    cursor.write_all(&[0xAB; 32])?;
                }
                Ok(())
            })
            .unwrap();
        wad_buffer.set_position(0);
        let mut wad = Wad::mount(wad_buffer).unwrap();

        // Hashtable resolves only the bin path - the asset is anonymous.
        let mut hashtable = WadHashtable::new().unwrap();
        hashtable.insert(bin_hash, "data/test.bin");

        let chunk_hashes: HashSet<u64> = [bin_hash, asset_hash].into_iter().collect();
        let discovered = scan_wad_bin_paths(&mut wad, &hashtable, &chunk_hashes, false);

        // The asset's real path is recovered from the bin's dependency link...
        assert_eq!(
            discovered.get(&asset_hash).map(|path| path.as_ref()),
            Some(asset_path)
        );
        // ...and the already-known bin hash is not duplicated into the overlay.
        assert!(!discovered.contains_key(&bin_hash));
    }

    /// Helper: serialize a `BinTree` to bytes.
    fn serialize_bin(tree: &BinTree) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        tree.to_writer(&mut buffer).unwrap();
        buffer.into_inner()
    }

    #[test]
    fn default_scan_follows_links_into_discovered_bins() {
        use league_toolkit::wad::{Wad, WadBuilder, WadChunkBuilder};
        use std::io::Write;

        // Chain: root.bin (known) -> child.bin (anonymous) -> asset.dds (anonymous).
        // The asset is only reachable by parsing child.bin, which is itself only found
        // by following root.bin's link - so this only works with transitive following.
        let child_path = "data/child.bin";
        let asset_path = "assets/deep/asset.dds";
        let root_bytes = serialize_bin(&BinTree::builder().dependency(child_path).build());
        let child_bytes = serialize_bin(&BinTree::builder().dependency(asset_path).build());

        let root_hash = hash_wad_path("data/root.bin");
        let child_hash = hash_wad_path(child_path);
        let asset_hash = hash_wad_path(asset_path);

        let mut wad_buffer = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(WadChunkBuilder::default().with_path("data/root.bin"))
            .with_chunk(WadChunkBuilder::default().with_path(child_path))
            .with_chunk(WadChunkBuilder::default().with_path(asset_path))
            .build_to_writer(&mut wad_buffer, |path_hash, cursor| {
                if path_hash == root_hash {
                    cursor.write_all(&root_bytes)?;
                } else if path_hash == child_hash {
                    cursor.write_all(&child_bytes)?;
                } else {
                    cursor.write_all(&[0xCD; 16])?;
                }
                Ok(())
            })
            .unwrap();
        wad_buffer.set_position(0);
        let mut wad = Wad::mount(wad_buffer).unwrap();

        // Only the root bin is known up front.
        let mut hashtable = WadHashtable::new().unwrap();
        hashtable.insert(root_hash, "data/root.bin");

        let chunk_hashes: HashSet<u64> = [root_hash, child_hash, asset_hash].into_iter().collect();
        let discovered = scan_wad_bin_paths(&mut wad, &hashtable, &chunk_hashes, false);

        assert_eq!(
            discovered.get(&child_hash).map(|path| path.as_ref()),
            Some(child_path)
        );
        assert_eq!(
            discovered.get(&asset_hash).map(|path| path.as_ref()),
            Some(asset_path)
        );
    }

    #[test]
    fn full_scan_finds_unreferenced_anonymous_bins() {
        use league_toolkit::wad::{Wad, WadBuilder, WadChunkBuilder};
        use std::io::Write;

        // An anonymous bin with no hashtable name that is not linked from anything.
        let asset_path = "assets/orphan/asset.dds";
        let bin_bytes = serialize_bin(&BinTree::builder().dependency(asset_path).build());
        let bin_hash = hash_wad_path("data/orphan.bin");
        let asset_hash = hash_wad_path(asset_path);

        let mut wad_buffer = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(WadChunkBuilder::default().with_path("data/orphan.bin"))
            .with_chunk(WadChunkBuilder::default().with_path(asset_path))
            .build_to_writer(&mut wad_buffer, |path_hash, cursor| {
                if path_hash == bin_hash {
                    cursor.write_all(&bin_bytes)?;
                } else {
                    cursor.write_all(&[0xEE; 16])?;
                }
                Ok(())
            })
            .unwrap();
        wad_buffer.set_position(0);
        let mut wad = Wad::mount(wad_buffer).unwrap();

        // Empty hashtable: nothing is known, so the bin is anonymous and unreferenced.
        let hashtable = WadHashtable::new().unwrap();
        let chunk_hashes: HashSet<u64> = [bin_hash, asset_hash].into_iter().collect();

        // The default (fast) scan cannot find an unnamed, unreferenced bin.
        let fast = scan_wad_bin_paths(&mut wad, &hashtable, &chunk_hashes, false);
        assert!(fast.is_empty());

        // The full scan magic-detects the bin and recovers the asset name.
        let full = scan_wad_bin_paths(&mut wad, &hashtable, &chunk_hashes, true);
        assert_eq!(
            full.get(&asset_hash).map(|path| path.as_ref()),
            Some(asset_path)
        );
    }
}
