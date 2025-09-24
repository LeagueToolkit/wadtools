mod hashtable;

use std::path::Path;

pub use hashtable::*;

pub fn format_chunk_path_hash(path_hash: u64) -> String {
    format!("{:016x}", path_hash)
}

pub fn is_hex_chunk_path(path: &Path) -> bool {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
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
