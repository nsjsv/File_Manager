use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use exif::{experimental::Writer as ExifWriter, Field, In, Tag, Value};
use file_index::{
    build_file_search_index, build_file_search_index_for_paths, file_search_index_exists,
    search_file_index, search_file_tree, search_file_tree_with_cancel, DirectoryErrorPolicy,
    FileSearchIndexOptions, FileSearchOptions, IndexError,
};
use tempfile::tempdir;

#[path = "index/ignore_rules.rs"]
mod ignore_rules;
#[path = "index/search.rs"]
mod search;

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
