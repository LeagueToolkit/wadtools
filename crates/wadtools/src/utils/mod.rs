mod hashtable;

pub use hashtable::*;

pub fn format_chunk_path_hash(path_hash: u64) -> String {
    format!("{:x}", path_hash)
}
