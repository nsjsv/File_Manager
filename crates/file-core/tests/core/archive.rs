use super::*;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};

#[tokio::test]
async fn create_zip_archive_preserves_selected_tree() {
    let dir = tempdir().unwrap();
    let source_dir = dir.path().join("source");
    fs::create_dir(&source_dir).unwrap();
    fs::create_dir(source_dir.join("nested")).unwrap();
    fs::write(source_dir.join("root.txt"), b"root").unwrap();
    fs::write(source_dir.join("nested/child.txt"), b"child").unwrap();
    let target = dir.path().join("bundle.zip");

    create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source_dir],
            target: target.clone(),
            format: ArchiveFormat::Zip,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: None,
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    let file = fs::File::open(target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    assert!(archive.by_name("source/").is_ok());
    assert_eq!(archive.by_name("source/root.txt").unwrap().size(), 4);
    assert_eq!(
        archive.by_name("source/nested/child.txt").unwrap().size(),
        5
    );
}

#[tokio::test]
async fn single_file_zip_round_trip_extracts_file_into_default_directory() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("note.txt");
    fs::write(&source, b"hello").unwrap();
    let target = dir.path().join("note.zip");

    create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source],
            target: target.clone(),
            format: ArchiveFormat::Zip,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: None,
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    let request = ArchiveExtractionRequest::from_archive_path(target, None).unwrap();
    let destination = extract_archive(request.clone(), tokio_util::sync::CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(destination, request.destination);
    assert_eq!(
        fs::read(request.destination.join("note.txt")).unwrap(),
        b"hello"
    );
}

#[tokio::test]
async fn create_tar_gz_archive_preserves_selected_file() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("note.txt");
    fs::write(&source, b"hello").unwrap();
    let target = dir.path().join("bundle.tar.gz");

    create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source],
            target: target.clone(),
            format: ArchiveFormat::TarGz,
            compression_level: ArchiveCompressionLevel::Fast,
            password: None,
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    let file = fs::File::open(target).unwrap();
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let names = archive
        .entries()
        .unwrap()
        .map(|entry| {
            entry
                .unwrap()
                .path()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["note.txt".to_owned()]);
}

#[cfg(unix)]
#[tokio::test]
async fn zip_rejects_non_utf8_entries_before_creating_target() {
    let dir = tempdir().unwrap();
    let first_name = OsString::from_vec(b"entry-\x80".to_vec());
    let second_name = OsString::from_vec(b"entry-\x81".to_vec());
    let first_source = dir.path().join(&first_name);
    let second_source = dir.path().join(&second_name);
    fs::write(&first_source, b"first").unwrap();
    fs::write(&second_source, b"second").unwrap();
    assert_ne!(first_name.as_bytes(), second_name.as_bytes());
    assert_eq!(first_name.to_string_lossy(), second_name.to_string_lossy());

    for (target_name, password) in [
        ("plain.zip", None),
        (
            "password-protected.zip",
            file_core::ArchivePassword::new("secret"),
        ),
    ] {
        let target = dir.path().join(target_name);
        let error = create_archive_with_progress(
            ArchiveCreationRequest {
                sources: vec![first_source.clone(), second_source.clone()],
                target: target.clone(),
                format: ArchiveFormat::Zip,
                compression_level: ArchiveCompressionLevel::Balanced,
                password,
            },
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap_err();

        match error {
            FileError::InvalidInput { path, message } => {
                assert_eq!(path, first_source);
                assert!(message.contains("UTF-8"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(!target.exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn tar_gz_preserves_non_utf8_entry_name() {
    let dir = tempdir().unwrap();
    let source_name = OsString::from_vec(b"entry-\x80".to_vec());
    let source = dir.path().join(&source_name);
    fs::write(&source, b"content").unwrap();
    let target = dir.path().join("bundle.tar.gz");

    create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source],
            target: target.clone(),
            format: ArchiveFormat::TarGz,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: None,
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap();

    let file = fs::File::open(target).unwrap();
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entry = archive.entries().unwrap().next().unwrap().unwrap();

    assert_eq!(entry.path_bytes().as_ref(), source_name.as_bytes());
}

#[tokio::test]
async fn create_archive_rejects_existing_target() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("note.txt");
    let target = dir.path().join("bundle.zip");
    fs::write(&source, b"hello").unwrap();
    fs::write(&target, b"taken").unwrap();

    let error = create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source],
            target: target.clone(),
            format: ArchiveFormat::Zip,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: None,
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap_err();

    match error {
        FileError::CreateFile { path, source } => {
            assert_eq!(path, target);
            assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn tar_gz_password_returns_invalid_input() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("note.txt");
    let target = dir.path().join("bundle.tar.gz");
    fs::write(&source, b"hello").unwrap();

    let error = create_archive_with_progress(
        ArchiveCreationRequest {
            sources: vec![source],
            target: target.clone(),
            format: ArchiveFormat::TarGz,
            compression_level: ArchiveCompressionLevel::Balanced,
            password: file_core::ArchivePassword::new("secret"),
        },
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await
    .unwrap_err();

    match error {
        FileError::InvalidInput { path, message } => {
            assert_eq!(path, target);
            assert!(message.contains("tar.gz"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn archive_password_debug_is_redacted() {
    let password = file_core::ArchivePassword::new("secret").unwrap();

    assert_eq!(format!("{password:?}"), "ArchivePassword(<redacted>)");
}
