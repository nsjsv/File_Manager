use file_core::{
    create_archive_with_progress, ArchiveCompressionLevel, ArchiveCreationRequest, ArchiveFormat,
};
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

use crate::model::{PreviewTreeDirectoryChildren, TextPreviewFormat};
use crate::text_preview_loading::PREVIEW_TEXT_LIMIT;

use super::*;

const TEST_MAX_PREVIEW_FILE_BYTES: u64 = 8 * 1024 * 1024;
const TEST_LARGE_MAX_PREVIEW_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[tokio::test]
async fn load_preview_reads_zip_archive_tree() {
    let temp_dir = tempdir().expect("temp dir");
    let archive_path = temp_dir.path().join("sample.zip");
    write_test_archive(&archive_path, ArchiveFormat::Zip).await;

    let preview_content = load_preview(
        archive_path.clone(),
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("zip archive preview");

    let PreviewContent::Archive { entries } = preview_content else {
        panic!("expected archive preview");
    };

    assert_preview_tree_entry(&entries[0], "src", FileKind::Directory, 0, None);
    assert_preview_tree_entry(&entries[1], "main.rs", FileKind::File, 1, Some(0));
    assert_preview_tree_entry(&entries[2], "README.md", FileKind::File, 0, None);
    assert!(entries[0].is_expanded);
    assert_eq!(entries[0].toggle_rotation_progress, 1.0);
}

#[tokio::test]
async fn load_preview_reports_recognized_unsupported_archive_format() {
    let temp_dir = tempdir().expect("temp dir");
    let archive_path = temp_dir.path().join("sample.XZ");
    std::fs::write(&archive_path, b"not an archive").expect("write archive fixture");

    let error = load_preview(
        archive_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect_err("unsupported archive should be identified");

    assert!(error.contains("This archive format is not supported yet"));
    assert!(error.contains(SUPPORTED_ARCHIVE_FORMAT_MESSAGE));
}

#[test]
fn unsupported_archive_classifier_does_not_overlap_supported_formats() {
    for path in [
        "sample.xz",
        "sample.BZ2",
        "sample.zst",
        "sample.deb",
        "sample.rpm",
    ] {
        assert!(
            is_recognized_unsupported_archive_path(Path::new(path)),
            "{path}"
        );
    }
    for path in [
        "sample.zip",
        "sample.tar",
        "sample.tar.gz",
        "sample.tgz",
        "sample.7z",
        "sample.rar",
        "sample.txt",
    ] {
        assert!(
            !is_recognized_unsupported_archive_path(Path::new(path)),
            "{path}"
        );
    }
}

#[tokio::test]
async fn load_preview_reads_directory_top_layer_only() {
    let temp_dir = tempdir().expect("temp dir");
    let nested_dir = temp_dir.path().join("src");
    std::fs::create_dir(&nested_dir).expect("create nested dir");
    std::fs::write(nested_dir.join("main.rs"), "fn main() {}\n").expect("write nested file");
    std::fs::write(temp_dir.path().join("README.md"), "# sample\n").expect("write readme");

    let preview_content = load_preview(
        temp_dir.path().to_path_buf(),
        FileKind::Directory,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("directory preview");

    let PreviewContent::Directory { entries } = preview_content else {
        panic!("expected directory preview");
    };

    assert_eq!(entries.len(), 2);
    assert_preview_tree_entry(&entries[0], "src", FileKind::Directory, 0, None);
    assert_preview_tree_entry(&entries[1], "README.md", FileKind::File, 0, None);
    assert_eq!(
        entries[0].filesystem_path.as_deref(),
        Some(nested_dir.as_path())
    );
    assert_eq!(
        entries[0].directory_children.as_ref(),
        Some(&PreviewTreeDirectoryChildren::Pending)
    );
    assert!(!entries[0].is_expanded);
    assert_eq!(entries[0].toggle_rotation_progress, 0.0);
}

#[tokio::test]
async fn load_directory_preview_children_reads_expanded_layer() {
    let temp_dir = tempdir().expect("temp dir");
    let nested_dir = temp_dir.path().join("src");
    std::fs::create_dir(&nested_dir).expect("create nested dir");
    std::fs::create_dir(nested_dir.join("deeper")).expect("create deeper dir");
    std::fs::write(nested_dir.join("main.rs"), "fn main() {}\n").expect("write nested file");

    let children = load_directory_preview_children(nested_dir, ScanOptions::default())
        .await
        .expect("directory preview children");
    let child_names = children
        .iter()
        .map(|entry| entry.name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(child_names, vec!["deeper".to_owned(), "main.rs".to_owned()]);
}

#[tokio::test]
async fn load_preview_reads_gzip_tar_archive_tree() {
    let temp_dir = tempdir().expect("temp dir");
    let archive_path = temp_dir.path().join("sample.tar.gz");
    write_test_archive(&archive_path, ArchiveFormat::TarGz).await;

    let preview_content = load_preview(
        archive_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("tar.gz archive preview");

    let PreviewContent::Archive { entries, .. } = preview_content else {
        panic!("expected archive preview");
    };

    assert_preview_tree_entry(&entries[0], "src", FileKind::Directory, 0, None);
    assert_preview_tree_entry(&entries[1], "main.rs", FileKind::File, 1, Some(0));
    assert_preview_tree_entry(&entries[2], "README.md", FileKind::File, 0, None);
}

#[tokio::test]
async fn load_preview_keeps_utf8_text_preview() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("note.txt");
    std::fs::write(&text_path, "hello\n").expect("write text file");

    let preview_content = load_preview(
        text_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("text preview");

    let PreviewContent::Text {
        rendered, format, ..
    } = preview_content
    else {
        panic!("expected text preview");
    };
    assert_eq!(rendered, "hello\n");
    assert_eq!(format, TextPreviewFormat::Plain);
}

#[tokio::test]
async fn load_preview_renders_markdown_text_preview() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("README.md");
    std::fs::write(&text_path, "# Title\n\nHello **world**.\n").expect("write markdown file");

    let preview_content = load_preview(
        text_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("markdown preview");

    let PreviewContent::Text {
        rendered, format, ..
    } = preview_content
    else {
        panic!("expected text preview");
    };
    assert_eq!(format, TextPreviewFormat::Markdown);
    assert_eq!(rendered, "# Title\n\nHello **world**.\n");
}

#[tokio::test]
async fn load_preview_chunks_plain_text_preview() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("large.txt");
    let content = numbered_line_range(0, 150);
    std::fs::write(&text_path, &content).expect("write text file");

    let preview_content = load_preview(
        text_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("text preview");

    let PreviewContent::Text {
        rendered,
        next_offset,
        loaded_line_count,
        line_limit_notice,
        ..
    } = preview_content
    else {
        panic!("expected text preview");
    };
    assert!(rendered.contains("line 49"));
    assert!(!rendered.contains("line 50"));
    assert_eq!(loaded_line_count, 50);
    assert!(next_offset.is_some());
    assert_eq!(line_limit_notice, None);
}

#[tokio::test]
async fn load_preview_rejects_file_over_configured_limit() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("single-line.txt");
    let content = "a".repeat(1025);
    std::fs::write(&text_path, content).expect("write text file");

    let error = load_preview(text_path, FileKind::File, ScanOptions::default(), 1024)
        .await
        .expect_err("file should be rejected");

    assert!(error.contains("File is too large to preview"));
}

#[tokio::test]
async fn load_preview_truncates_long_markdown_on_utf8_boundary() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("unicode.md");
    let content = "€".repeat(PREVIEW_TEXT_LIMIT);
    std::fs::write(&text_path, content).expect("write text file");

    let preview_content = load_preview(
        text_path,
        FileKind::File,
        ScanOptions::default(),
        TEST_LARGE_MAX_PREVIEW_FILE_BYTES,
    )
    .await
    .expect("text preview");

    let PreviewContent::Text { rendered, .. } = preview_content else {
        panic!("expected text preview");
    };
    assert_eq!(rendered.len(), PREVIEW_TEXT_LIMIT - 1);
    assert!(rendered.is_char_boundary(rendered.len()));
}

fn numbered_line_range(start: usize, end: usize) -> String {
    let mut content = String::new();
    for index in start..end {
        content.push_str(&format!("line {index}\n"));
    }
    content
}

async fn write_test_archive(path: &Path, format: ArchiveFormat) {
    let source_dir = path
        .parent()
        .expect("archive parent")
        .join("archive-source");
    std::fs::create_dir_all(source_dir.join("src")).expect("create source dir");
    std::fs::write(source_dir.join("src").join("main.rs"), "fn main() {}\n")
        .expect("write nested source");
    std::fs::write(source_dir.join("README.md"), "# sample\n").expect("write readme source");

    create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source_dir.join("src"), source_dir.join("README.md")],
            target: path.to_path_buf(),
            format,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: None,
        },
        CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("create test archive");
}

fn assert_preview_tree_entry(
    entry: &PreviewTreeEntry,
    name: &str,
    kind: FileKind,
    depth: usize,
    parent: Option<usize>,
) {
    assert_eq!(entry.name, name);
    assert_eq!(entry.kind, kind);
    assert_eq!(entry.depth, depth);
    assert_eq!(entry.parent, parent);
}
