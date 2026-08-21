//! The `-f`, `-x` and `--hash` filters and their `-v` inversion, as one rule each
//! for every command.

use fancy_regex::Regex;
use league_toolkit::{file::LeagueFileKind, wad::WadHash};

/// Returns true if the chunk should be skipped based on the pattern filter.
pub(crate) fn should_skip_pattern(
    path: &str,
    filter_pattern: Option<&Regex>,
    filter_invert: bool,
) -> bool {
    if let Some(regex) = filter_pattern {
        let matched = regex.is_match(path).unwrap_or(false);
        return matched == filter_invert;
    }
    false
}

/// Returns true if the chunk should be skipped based on the hash filter.
pub(crate) fn should_skip_hash(
    path_hash: WadHash,
    hash_filter: Option<&[WadHash]>,
    filter_invert: bool,
) -> bool {
    hash_filter.is_some_and(|hashes| hashes.contains(&path_hash) == filter_invert)
}

/// Returns true if the chunk should be skipped based on the type filter.
pub(crate) fn should_skip_type(
    chunk_kind: LeagueFileKind,
    filter_type: Option<&[LeagueFileKind]>,
    filter_invert: bool,
) -> bool {
    filter_type.is_some_and(|filter| filter.contains(&chunk_kind) == filter_invert)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> Regex {
        Regex::new(pattern).unwrap()
    }

    // --- should_skip_pattern tests ---

    #[test]
    fn no_pattern_never_skips() {
        assert!(!should_skip_pattern("anything.dds", None, false));
        assert!(!should_skip_pattern("anything.dds", None, true));
    }

    #[test]
    fn pattern_includes_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(!should_skip_pattern("textures/foo.dds", Some(&re), false));
    }

    #[test]
    fn pattern_excludes_non_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(should_skip_pattern("sounds/bar.wav", Some(&re), false));
    }

    #[test]
    fn pattern_inverted_excludes_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(should_skip_pattern("textures/foo.dds", Some(&re), true));
    }

    #[test]
    fn pattern_inverted_includes_non_matching() {
        let re = regex(r"(?i)\.dds$");
        assert!(!should_skip_pattern("sounds/bar.wav", Some(&re), true));
    }

    // --- should_skip_hash tests ---

    #[test]
    fn no_hash_filter_never_skips() {
        assert!(!should_skip_hash(WadHash(1), None, false));
        assert!(!should_skip_hash(WadHash(1), None, true));
    }

    #[test]
    fn hash_filter_keeps_listed_and_drops_the_rest() {
        let hashes = [WadHash(1), WadHash(2)];
        assert!(!should_skip_hash(WadHash(1), Some(&hashes), false));
        assert!(should_skip_hash(WadHash(3), Some(&hashes), false));
    }

    #[test]
    fn hash_filter_inverted_drops_listed_and_keeps_the_rest() {
        let hashes = [WadHash(1), WadHash(2)];
        assert!(should_skip_hash(WadHash(1), Some(&hashes), true));
        assert!(!should_skip_hash(WadHash(3), Some(&hashes), true));
    }

    // --- should_skip_type tests ---

    #[test]
    fn no_type_filter_never_skips() {
        assert!(!should_skip_type(LeagueFileKind::Png, None, false));
        assert!(!should_skip_type(LeagueFileKind::Png, None, true));
    }

    #[test]
    fn type_filter_includes_matching() {
        let types = [LeagueFileKind::Png];
        assert!(!should_skip_type(LeagueFileKind::Png, Some(&types), false));
    }

    #[test]
    fn type_filter_excludes_non_matching() {
        let types = [LeagueFileKind::Png];
        assert!(should_skip_type(LeagueFileKind::Jpeg, Some(&types), false));
    }

    #[test]
    fn type_filter_inverted_excludes_matching() {
        let types = [LeagueFileKind::Png];
        assert!(should_skip_type(LeagueFileKind::Png, Some(&types), true));
    }

    #[test]
    fn type_filter_inverted_includes_non_matching() {
        let types = [LeagueFileKind::Png];
        assert!(!should_skip_type(LeagueFileKind::Jpeg, Some(&types), true));
    }
}
