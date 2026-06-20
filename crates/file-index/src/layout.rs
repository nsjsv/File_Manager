use std::path::{Path, PathBuf};

use crate::search::path_encoding::path_to_bytes;

pub fn search_index_dir_for_root(index_base_dir: &Path, root: &Path) -> PathBuf {
    index_base_dir.join(profile_root_key(root))
}

fn profile_root_key(root: &Path) -> String {
    hex_encode(&path_to_bytes(root))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_index_dir_uses_path_bytes_hex_key() {
        assert_eq!(
            search_index_dir_for_root(Path::new("/cache/index"), Path::new("/tmp/root")),
            PathBuf::from("/cache/index/2f746d702f726f6f74")
        );
    }
}
