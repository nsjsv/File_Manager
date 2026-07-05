use std::fs;
use std::io::{Cursor, Seek, SeekFrom};
use std::time::{Duration, Instant};

use exif::{experimental::Writer as ExifWriter, Field, In, Tag, Value};
use file_index::{
    build_file_search_index, search_file_index, FileSearchIndexOptions, FileSearchOptions,
    SearchMode, SearchResultSource,
};
use tempfile::tempdir;

const HOT_TARGET: Duration = Duration::from_millis(50);
const COLD_TARGET: Duration = Duration::from_millis(200);

#[derive(Clone)]
struct QueryCase {
    name: &'static str,
    query: &'static str,
    options: FileSearchOptions,
    expected_source: SearchResultSource,
}

#[tokio::test]
#[ignore]
async fn query_modes_meet_hot_and_cold_latency_targets() {
    let root_dir = tempdir().unwrap();
    let index_dir = tempdir().unwrap();
    populate_query_fixture(root_dir.path());

    build_file_search_index(root_dir.path(), index_dir.path(), all_index_options(true))
        .await
        .unwrap();

    let cases = [
        QueryCase {
            name: "files",
            query: "quarterly-plan",
            options: files_search_options(true, 20),
            expected_source: SearchResultSource::Files,
        },
        QueryCase {
            name: "contents",
            query: "runway",
            options: content_search_options(true, 20),
            expected_source: SearchResultSource::Contents,
        },
        QueryCase {
            name: "media",
            query: "sunset",
            options: media_search_options(true, 20),
            expected_source: SearchResultSource::Media,
        },
        QueryCase {
            name: "all",
            query: "roadmap",
            options: all_search_options(true, 20),
            expected_source: SearchResultSource::Contents,
        },
    ];

    for case in cases {
        let cold = measure_cold_query(root_dir.path(), index_dir.path(), &case).await;
        let hot = measure_hot_query(root_dir.path(), index_dir.path(), &case).await;
        println!(
            "{} cold={}ms hot={}ms",
            case.name,
            cold.as_millis(),
            hot.as_millis()
        );
        assert!(
            cold <= COLD_TARGET,
            "{} cold query exceeded target: {:?}",
            case.name,
            cold
        );
        assert!(
            hot <= HOT_TARGET,
            "{} hot query exceeded target: {:?}",
            case.name,
            hot
        );
    }
}

async fn measure_cold_query(
    root: &std::path::Path,
    index_dir: &std::path::Path,
    case: &QueryCase,
) -> Duration {
    let mut samples = Vec::new();
    for _ in 0..3 {
        file_index::search::clear_search_query_cache_for_tests();
        let started_at = Instant::now();
        let outcome = search_file_index(index_dir, root, case.query, case.options.clone())
            .await
            .unwrap();
        samples.push(started_at.elapsed());
        assert!(
            !outcome.matches.is_empty(),
            "{} returned no matches",
            case.name
        );
        assert_eq!(outcome.matches[0].source, case.expected_source);
    }
    median_duration(samples)
}

async fn measure_hot_query(
    root: &std::path::Path,
    index_dir: &std::path::Path,
    case: &QueryCase,
) -> Duration {
    file_index::search::clear_search_query_cache_for_tests();
    let warm = search_file_index(index_dir, root, case.query, case.options.clone())
        .await
        .unwrap();
    assert!(
        !warm.matches.is_empty(),
        "{} warmup returned no matches",
        case.name
    );

    let mut samples = Vec::new();
    for _ in 0..9 {
        let started_at = Instant::now();
        let outcome = search_file_index(index_dir, root, case.query, case.options.clone())
            .await
            .unwrap();
        samples.push(started_at.elapsed());
        assert!(
            !outcome.matches.is_empty(),
            "{} hot query returned no matches",
            case.name
        );
        assert_eq!(outcome.matches[0].source, case.expected_source);
    }
    median_duration(samples)
}

fn median_duration(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn populate_query_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("projects/reporting")).unwrap();
    fs::create_dir_all(root.join("archive/notes")).unwrap();

    fs::write(
        root.join("projects/reporting/quarterly-plan.md"),
        "roadmap body runway alignment",
    )
    .unwrap();
    fs::write(root.join("meeting-notes.md"), "runway checklist for launch").unwrap();
    fs::write(root.join("roadmap.md"), "roadmap body and milestones").unwrap();

    for index in 0..1_200 {
        let bucket = format!("bucket-{:02}", index % 24);
        let dir = root.join("archive/notes").join(bucket);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("note-{index:04}.txt")),
            format!("background filler line {index}"),
        )
        .unwrap();
    }

    let media_path = root.join("holiday-photo.png");
    fs::write(
        &media_path,
        png_with_exif_description("sunset ridge overlook"),
    )
    .unwrap();
}

fn png_with_exif_description(description: &str) -> Vec<u8> {
    let field = Field {
        tag: Tag::ImageDescription,
        ifd_num: In::PRIMARY,
        value: Value::Ascii(vec![description.as_bytes().to_vec()]),
    };
    let mut writer = ExifWriter::new();
    let mut exif = Cursor::new(Vec::new());
    writer.push_field(&field);
    writer.write(&mut exif, false).unwrap();

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\x0d\x0a\x1a\x0a");
    append_png_chunk(&mut png, b"IHDR", &minimal_png_ihdr());
    exif.seek(SeekFrom::Start(0)).unwrap();
    append_png_chunk(&mut png, b"eXIf", &exif.into_inner());
    append_png_chunk(&mut png, b"IDAT", &minimal_png_idat());
    append_png_chunk(&mut png, b"IEND", &[]);
    png
}

fn minimal_png_ihdr() -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    ihdr
}

fn minimal_png_idat() -> Vec<u8> {
    vec![
        0x78, 0x01, 0x01, 0x04, 0x00, 0xfb, 0xff, 0x00, 0, 0, 0, 0, 0x00, 0x05, 0x00, 0x01,
    ]
}

fn append_png_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    png.extend_from_slice(&png_crc(kind, data).to_be_bytes());
}

fn png_crc(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn files_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: SearchMode::Files,
        ..query_policy_base(include_hidden, limit)
    }
}

fn content_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: SearchMode::Contents,
        ..query_policy_base(include_hidden, limit)
    }
}

fn media_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: SearchMode::Media,
        ..query_policy_base(include_hidden, limit)
    }
}

fn all_search_options(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        mode: SearchMode::All,
        ..query_policy_base(include_hidden, limit)
    }
}

fn query_policy_base(include_hidden: bool, limit: usize) -> FileSearchOptions {
    FileSearchOptions {
        include_hidden,
        limit,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        ..FileSearchOptions::default()
    }
}

fn all_index_options(include_hidden: bool) -> FileSearchIndexOptions {
    FileSearchIndexOptions {
        include_hidden,
        media_metadata_scope: file_index::MediaMetadataScope::All,
        ..FileSearchIndexOptions::default()
    }
}
