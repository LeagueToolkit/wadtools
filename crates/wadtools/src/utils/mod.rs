pub mod config;
mod hashtable;

use camino::{Utf8Path, Utf8PathBuf};
use fancy_regex::Regex;

pub use hashtable::*;

/// Resolves a list of input strings into a flat list of WAD file paths.
/// Splits any semicolon-delimited entries and flattens the result.
pub fn resolve_inputs(inputs: &[String]) -> Vec<String> {
    inputs
        .iter()
        .flat_map(|entry| entry.split(';'))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Returns true if a file name looks like a WAD archive.
///
/// Matches both plain `.wad` files and target-suffixed variants such as
/// `.wad.client` / `.wad.mobile` (any `*.wad.*`). Case-insensitive.
pub fn is_wad_file(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".wad") || lower.contains(".wad.")
}

/// Expands directory inputs into the WAD files they contain.
///
/// Each entry is treated as follows:
/// - a directory is recursively walked and every contained WAD file (see [`is_wad_file`])
///   is emitted, sorted for deterministic ordering;
/// - any other path (a file, or something that does not exist) is passed through unchanged
///   so the caller still surfaces a sensible error when opening it.
///
/// This lets a folder dropped onto the executable (or picked via the Explorer folder
/// context menu) expand to all the WADs inside it.
pub fn expand_wad_inputs(inputs: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(inputs.len());
    for input in inputs {
        if Utf8Path::new(&input).is_dir() {
            let mut found: Vec<String> = walkdir::WalkDir::new(&input)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_type().is_file())
                .filter(|entry| entry.file_name().to_str().is_some_and(is_wad_file))
                .filter_map(|entry| entry.path().to_str().map(|s| s.to_string()))
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(input);
        }
    }
    out
}

/// Creates a filter pattern from an optional regex string.
/// Defaults to case-insensitive matching unless the user explicitly sets (?i) or (?-i).
pub fn create_filter_pattern(pattern: Option<String>) -> eyre::Result<Option<Regex>> {
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

pub fn format_chunk_path_hash(path_hash: u64) -> String {
    format!("{:016x}", path_hash)
}

pub fn is_hex_chunk_path(path: &Utf8Path) -> bool {
    let file_name = path.file_name().unwrap_or("");
    file_name.len() == 16 && file_name.chars().all(|c| c.is_ascii_hexdigit())
}

/// Truncates a string in the middle
pub fn truncate_middle(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }
    let keep = max_len - 3;
    let left = keep / 2;
    let right = keep - left;
    let mut left_iter = input.chars();
    let mut left_str = String::with_capacity(left);
    for _ in 0..left {
        if let Some(c) = left_iter.next() {
            left_str.push(c);
        }
    }
    let mut right_iter = input.chars().rev();
    let mut right_str = String::with_capacity(right);
    for _ in 0..right {
        if let Some(c) = right_iter.next() {
            right_str.push(c);
        }
    }
    right_str = right_str.chars().rev().collect();
    format!("{}...{}", left_str, right_str)
}

/// Returns the directory the pre-mimir wadtools used for the CommunityDragon
/// `hashes.*.txt` files. Retained only so we can clean those files up; hash
/// resolution now uses the mimir shared cache (see [`crate::utils::WadHashtable`]).
///
/// On Windows this was the user's Documents folder
/// (`Documents/LeagueToolkit/wad_hashtables`); on other platforms the
/// platform data directory via `directories_next`.
pub fn legacy_hashtable_dir() -> Option<Utf8PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(mut doc_dir) = dirs_next::document_dir() {
            doc_dir.push("LeagueToolkit");
            doc_dir.push("wad_hashtables");
            return Utf8PathBuf::from_path_buf(doc_dir).ok();
        }
    }

    if let Some(proj) = directories_next::ProjectDirs::from("io", "LeagueToolkit", "wadtools") {
        let mut path = proj.data_dir().to_path_buf();
        path.push("wad_hashtables");
        return Utf8PathBuf::from_path_buf(path).ok();
    }

    None
}

/// The CommunityDragon text files the old download flow wrote into the legacy
/// hashtable directory.
const LEGACY_HASHTABLE_FILES: &[&str] = &["hashes.game.txt", "hashes.lcu.txt"];

/// Removes the old CommunityDragon `hashes.*.txt` files now that resolution is
/// served from the mimir shared cache.
///
/// Best-effort and quiet: only the files we know we wrote are deleted (any custom
/// files the user placed there are left alone), and the directory is removed only
/// if it ends up empty. Errors are logged at debug level and never surfaced.
pub fn cleanup_legacy_hashtables() {
    let Some(dir) = legacy_hashtable_dir() else {
        return;
    };
    if !dir.exists() {
        return;
    }

    for name in LEGACY_HASHTABLE_FILES {
        let file = dir.join(name);
        if !file.exists() {
            continue;
        }
        match std::fs::remove_file(file.as_std_path()) {
            Ok(()) => tracing::info!("removed legacy hashtable file {file}"),
            Err(error) => tracing::debug!("could not remove legacy hashtable file {file}: {error}"),
        }
    }

    // Drop the directory only when it is now empty, so we never delete unrelated
    // files a user may have added.
    let is_empty = std::fs::read_dir(dir.as_std_path())
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false);
    if is_empty {
        match std::fs::remove_dir(dir.as_std_path()) {
            Ok(()) => tracing::info!("removed empty legacy hashtable directory {dir}"),
            Err(error) => {
                tracing::debug!("could not remove legacy hashtable directory {dir}: {error}")
            }
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_input() {
        assert_eq!(resolve_inputs(&s(&["a.wad.client"])), vec!["a.wad.client"]);
    }

    #[test]
    fn repeated_inputs() {
        assert_eq!(
            resolve_inputs(&s(&["a.wad", "b.wad"])),
            vec!["a.wad", "b.wad"]
        );
    }

    #[test]
    fn semicolon_delimited() {
        assert_eq!(
            resolve_inputs(&s(&["a.wad;b.wad;c.wad"])),
            vec!["a.wad", "b.wad", "c.wad"]
        );
    }

    #[test]
    fn semicolon_with_whitespace() {
        assert_eq!(
            resolve_inputs(&s(&["a.wad ; b.wad"])),
            vec!["a.wad", "b.wad"]
        );
    }

    #[test]
    fn semicolon_skips_empty() {
        assert_eq!(
            resolve_inputs(&s(&["a.wad;;b.wad;"])),
            vec!["a.wad", "b.wad"]
        );
    }

    #[test]
    fn mixed_repeated_and_semicolon() {
        assert_eq!(
            resolve_inputs(&s(&["a.wad;b.wad", "c.wad"])),
            vec!["a.wad", "b.wad", "c.wad"]
        );
    }

    #[test]
    fn empty_input() {
        let result = resolve_inputs(&s(&[]));
        assert!(result.is_empty());
    }

    #[test]
    fn is_wad_file_matches_variants() {
        assert!(is_wad_file("Aatrox.wad"));
        assert!(is_wad_file("Aatrox.wad.client"));
        assert!(is_wad_file("Aatrox.wad.mobile"));
        assert!(is_wad_file("AATROX.WAD.CLIENT")); // case-insensitive
    }

    #[test]
    fn is_wad_file_rejects_non_wad() {
        assert!(!is_wad_file("foo.txt"));
        assert!(!is_wad_file("wad")); // no extension
        assert!(!is_wad_file("foo.wadx")); // not a wad extension
        assert!(!is_wad_file("mywad")); // substring but not extension
    }

    #[test]
    fn expand_passes_through_files() {
        // Non-existent / non-directory paths are passed through unchanged.
        assert_eq!(
            expand_wad_inputs(s(&["a.wad.client", "b.wad"])),
            vec!["a.wad.client", "b.wad"]
        );
    }

    #[test]
    fn expand_directory_collects_wads() {
        let dir = std::env::temp_dir().join(format!("wadtools_expand_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("Aatrox.wad.client"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("nested").join("Ahri.wad"), b"x").unwrap();

        let dir_str = dir.to_str().unwrap().to_string();
        let result = expand_wad_inputs(vec![dir_str]);

        assert_eq!(result.len(), 2, "expected two wad files, got {result:?}");
        assert!(result.iter().any(|p| p.ends_with("Aatrox.wad.client")));
        assert!(result.iter().any(|p| p.ends_with("Ahri.wad")));
        assert!(!result.iter().any(|p| p.ends_with("notes.txt")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
