use std::path::{Path, PathBuf};

use crate::search::path_encoding::path_storage_key;

pub fn search_index_dir_for_root(index_base_dir: &Path, root: &Path) -> PathBuf {
    index_base_dir.join(path_storage_key(root))
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
