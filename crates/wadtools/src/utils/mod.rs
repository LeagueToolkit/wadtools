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

/// Returns the default directory where wad hashtables should be looked up.
/// On Windows, prefers the user's Documents folder: Documents/LeagueToolkit/wad_hashtables
/// On other platforms, uses platform-appropriate data directory via directories_next.
pub fn default_hashtable_dir() -> Option<Utf8PathBuf> {
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
}
