use std::io::Write;

use tempfile::tempdir;
use zip::write::SimpleFileOptions;

use crate::model::{PreviewTreeDirectoryChildren, TextPreviewFormat};

use super::*;

#[tokio::test]
async fn load_preview_reads_zip_archive_tree() {
    let temp_dir = tempdir().expect("temp dir");
    let archive_path = temp_dir.path().join("sample.zip");
    write_zip_archive(&archive_path);

    let preview_content =
        load_preview(archive_path.clone(), FileKind::File, ScanOptions::default())
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
    write_gzip_tar_archive(&archive_path);

    let preview_content = load_preview(archive_path, FileKind::File, ScanOptions::default())
        .await
        .expect("tar.gz archive preview");

    let PreviewContent::Archive { entries, .. } = preview_content else {
        panic!("expected archive preview");
    };

    assert_preview_tree_entry(&entries[0], "nested", FileKind::Directory, 0, None);
    assert_preview_tree_entry(&entries[1], "file.txt", FileKind::File, 1, Some(0));
}

#[tokio::test]
async fn load_preview_keeps_utf8_text_preview() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("note.txt");
    std::fs::write(&text_path, "hello\n").expect("write text file");

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
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

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
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
async fn load_preview_reads_text_beyond_first_100_lines() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("large.txt");
    let content = numbered_line_range(0, 150);
    std::fs::write(&text_path, &content).expect("write text file");

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
        .await
        .expect("text preview");

    let PreviewContent::Text { rendered, .. } = preview_content else {
        panic!("expected text preview");
    };
    assert_eq!(rendered, content);
}

#[tokio::test]
async fn load_preview_reads_ten_thousand_numbered_lines() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("ten-thousand-lines.txt");
    let content = numbered_padded_line_range(0, 10_000, 96);
    std::fs::write(&text_path, &content).expect("write text file");

    assert!(content.len() > 256 * 1024);

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
        .await
        .expect("text preview");

    let PreviewContent::Text { rendered, .. } = preview_content else {
        panic!("expected text preview");
    };
    assert_eq!(rendered, content);
    assert!(rendered.contains("line 9999: "));
}

#[tokio::test]
async fn load_preview_limits_long_text_line_bytes() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("single-line.txt");
    let content = "a".repeat(PREVIEW_TEXT_LIMIT + 32);
    std::fs::write(&text_path, content).expect("write text file");

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
        .await
        .expect("text preview");

    let PreviewContent::Text { rendered, .. } = preview_content else {
        panic!("expected text preview");
    };
    assert_eq!(rendered.len(), PREVIEW_TEXT_LIMIT);
}

#[tokio::test]
async fn load_preview_truncates_long_text_on_utf8_boundary() {
    let temp_dir = tempdir().expect("temp dir");
    let text_path = temp_dir.path().join("unicode.txt");
    let content = "€".repeat(PREVIEW_TEXT_LIMIT);
    std::fs::write(&text_path, content).expect("write text file");

    let preview_content = load_preview(text_path, FileKind::File, ScanOptions::default())
        .await
        .expect("text preview");

    let PreviewContent::Text { rendered, .. } = preview_content else {
        panic!("expected text preview");
    };
    assert_eq!(rendered.as_bytes().len(), PREVIEW_TEXT_LIMIT - 1);
    assert!(rendered.is_char_boundary(rendered.len()));
}

#[test]
fn parse_seven_zip_listing_reads_directory_markers() {
    let members = parse_seven_zip_listing(
        r#"
Path = archive.rar
Type = Rar

----------
Path = src
Folder = +
Attributes = D_ drwxr-xr-x

Path = src/main.rs
Folder = -
Attributes = A_ -rw-r--r--

Path = docs\guide.md
Folder = -
Attributes = A_ -rw-r--r--
"#,
    );

    assert_eq!(members.len(), 3);
    assert_eq!(members[0].path, "src");
    assert_eq!(members[0].kind, FileKind::Directory);
    assert_eq!(members[1].path, "src/main.rs");
    assert_eq!(members[1].kind, FileKind::File);
    assert_eq!(members[2].path, "docs\\guide.md");
    assert_eq!(members[2].kind, FileKind::File);
}

fn numbered_line_range(start: usize, end: usize) -> String {
    let mut content = String::new();
    for index in start..end {
        content.push_str(&format!("line {index}\n"));
    }
    content
}

fn numbered_padded_line_range(start: usize, end: usize, padding: usize) -> String {
    let filler = "x".repeat(padding);
    let mut content = String::new();
    for index in start..end {
        content.push_str(&format!("line {index}: {filler}\n"));
    }
    content
}

fn write_zip_archive(path: &Path) {
    let file = File::create(path).expect("create zip file");
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive.add_directory("src/", options).expect("zip dir");
    archive
        .start_file("src/main.rs", options)
        .expect("zip nested file");
    archive.write_all(b"fn main() {}\n").expect("zip content");
    archive
        .start_file("README.md", options)
        .expect("zip root file");
    archive.write_all(b"# sample\n").expect("zip readme");
    archive.finish().expect("finish zip");
}

fn write_gzip_tar_archive(path: &Path) {
    let file = File::create(path).expect("create tar.gz file");
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    let bytes = b"hello\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, "nested/file.txt", &bytes[..])
        .expect("tar nested file");
    let encoder = archive.into_inner().expect("finish tar");
    encoder.finish().expect("finish gzip");
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
