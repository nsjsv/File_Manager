use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn app_ui_does_not_open_index_service_or_catalog_directly() {
    let source_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "IndexService::open",
        "IndexServiceCore::open",
        "ProfileStore",
        "build_file_search_index",
        "search_file_index",
        "file_search_index_status",
        "clear_file_search_index_failures",
        "remove_file_search_index",
    ];

    for source_file in rust_source_files(&source_dir) {
        let source = fs::read_to_string(&source_file).unwrap();
        for pattern in forbidden {
            assert!(
                !source.contains(pattern),
                "{} must use the index daemon client boundary instead of `{pattern}`",
                source_file.display()
            );
        }
    }
}

fn rust_source_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_source_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}
