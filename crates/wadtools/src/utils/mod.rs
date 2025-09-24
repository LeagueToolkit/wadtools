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
