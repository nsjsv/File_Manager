use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use exif::{In, Reader as ExifReader, Tag};
use file_core::{supported_media_kind_for_path, FileKind, ScanWarning, SupportedMediaKind};
use serde::Deserialize;

use super::catalog::SearchCatalogRecord;
use super::types::{MediaExifField, MediaSearchKind, MediaSearchMetadata};

const BINARY_SNIFF_BYTES: usize = 8192;
const FFMPEG_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(crate) struct ExtractedTextDocument {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) name: String,
    pub(crate) content: String,
    pub(crate) truncated: bool,
    pub(crate) rank_hint: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedMediaDocument {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) name: String,
    pub(crate) metadata: MediaSearchMetadata,
    pub(crate) searchable_text: String,
    pub(crate) rank_hint: u64,
}

pub(crate) fn extract_text_documents(
    records: &[SearchCatalogRecord],
    max_file_bytes: u64,
) -> (Vec<ExtractedTextDocument>, Vec<ScanWarning>) {
    let mut documents = Vec::new();
    let mut warnings = Vec::new();
    for record in records {
        match extract_text_document(record, max_file_bytes) {
            Ok(Some(document)) => documents.push(document),
            Ok(None) => {}
            Err(warning) => warnings.push(warning),
        }
    }
    (documents, warnings)
}

pub(crate) fn extract_media_documents(
    records: &[SearchCatalogRecord],
) -> (Vec<ExtractedMediaDocument>, Vec<ScanWarning>) {
    let mut documents = Vec::new();
    let mut warnings = Vec::new();
    for record in records {
        match extract_media_document(record) {
            Ok(Some(document)) => documents.push(document),
            Ok(None) => {}
            Err(warning) => warnings.push(warning),
        }
    }
    (documents, warnings)
}

fn extract_text_document(
    record: &SearchCatalogRecord,
    max_file_bytes: u64,
) -> Result<Option<ExtractedTextDocument>, ScanWarning> {
    if record.kind != FileKind::File || !is_text_index_candidate(&record.path) {
        return Ok(None);
    }

    let bytes = fs::read(&record.path).map_err(|error| ScanWarning {
        path: record.path.clone(),
        message: error.to_string(),
    })?;
    if looks_binary(&bytes) {
        return Ok(None);
    }

    let max_len = usize::try_from(max_file_bytes).unwrap_or(usize::MAX);
    let truncated = bytes.len() > max_len;
    let indexed_bytes = if truncated { &bytes[..max_len] } else { &bytes };
    let content = String::from_utf8_lossy(indexed_bytes).into_owned();
    if content.trim().is_empty() {
        return Ok(None);
    }

    Ok(Some(ExtractedTextDocument {
        path: record.path.clone(),
        relative_path: record.relative_path.clone(),
        name: record.name.to_string_lossy().into_owned(),
        content,
        truncated,
        rank_hint: record.size_bytes.unwrap_or_default(),
    }))
}

fn is_text_index_candidate(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rs"
            | "toml"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "html"
            | "css"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "sh"
            | "fish"
            | "zsh"
            | "sql"
            | "csv"
            | "log"
    )
}

fn looks_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(BINARY_SNIFF_BYTES)];
    sample.contains(&0)
}

fn extract_media_document(
    record: &SearchCatalogRecord,
) -> Result<Option<ExtractedMediaDocument>, ScanWarning> {
    if record.kind != FileKind::File {
        return Ok(None);
    }
    let Some(media_kind) = supported_media_kind_for_path(&record.path) else {
        return Ok(None);
    };
    let Some(mut metadata) =
        media_metadata_for_path(&record.path, media_kind).map_err(|error| ScanWarning {
            path: record.path.clone(),
            message: error.to_string(),
        })?
    else {
        return Ok(None);
    };
    let ffprobe = ffprobe_metadata(&record.path);
    if let Some(ffprobe) = ffprobe {
        metadata.width = metadata.width.or(ffprobe.width);
        metadata.height = metadata.height.or(ffprobe.height);
        metadata.duration_ms = metadata.duration_ms.or(ffprobe.duration_ms);
        metadata.codec = metadata.codec.or(ffprobe.codec);
        metadata.title = metadata.title.or(ffprobe.title);
        metadata.artist = metadata.artist.or(ffprobe.artist);
    }
    let name = record.name.to_string_lossy().into_owned();
    let searchable_text = media_search_text(&name, &metadata);

    Ok(Some(ExtractedMediaDocument {
        path: record.path.clone(),
        relative_path: record.relative_path.clone(),
        name,
        metadata,
        searchable_text,
        rank_hint: record.size_bytes.unwrap_or_default(),
    }))
}

fn media_metadata_for_path(
    path: &Path,
    kind: SupportedMediaKind,
) -> Result<Option<MediaSearchMetadata>, image::ImageError> {
    match kind {
        SupportedMediaKind::Image => {
            let (width, height) = image::image_dimensions(path)?;
            Ok(Some(MediaSearchMetadata {
                media_kind: MediaSearchKind::Image,
                width: Some(width),
                height: Some(height),
                duration_ms: None,
                codec: None,
                title: None,
                artist: None,
                exif: image_exif_fields(path),
            }))
        }
        SupportedMediaKind::Audio => Ok(Some(MediaSearchMetadata {
            media_kind: MediaSearchKind::Audio,
            width: None,
            height: None,
            duration_ms: None,
            codec: None,
            title: None,
            artist: None,
            exif: Vec::new(),
        })),
        SupportedMediaKind::Video => Ok(Some(MediaSearchMetadata {
            media_kind: MediaSearchKind::Video,
            width: None,
            height: None,
            duration_ms: None,
            codec: None,
            title: None,
            artist: None,
            exif: Vec::new(),
        })),
    }
}

fn media_search_text(name: &str, metadata: &MediaSearchMetadata) -> String {
    let exif_values = metadata.exif.iter().map(|field| field.value.as_str());
    [
        Some(name),
        metadata.codec.as_deref(),
        metadata.title.as_deref(),
        metadata.artist.as_deref(),
    ]
    .into_iter()
    .flatten()
    .chain(exif_values)
    .chain(std::iter::once(match metadata.media_kind {
        MediaSearchKind::Image => "image photo picture",
        MediaSearchKind::Audio => "audio music song",
        MediaSearchKind::Video => "video movie clip",
    }))
    .collect::<Vec<_>>()
    .join(" ")
}

fn image_exif_fields(path: &Path) -> Vec<MediaExifField> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };
    let mut reader = BufReader::new(file);
    let exif = match ExifReader::new().read_from_container(&mut reader) {
        Ok(exif) => exif,
        Err(_) => return Vec::new(),
    };

    [
        ("Camera make", Tag::Make),
        ("Camera model", Tag::Model),
        ("Captured", Tag::DateTimeOriginal),
        ("Lens", Tag::LensModel),
        ("Description", Tag::ImageDescription),
        ("Software", Tag::Software),
        ("Artist", Tag::Artist),
        ("Copyright", Tag::Copyright),
    ]
    .into_iter()
    .filter_map(|(label, tag)| {
        let value = exif
            .get_field(tag, In::PRIMARY)
            .map(|field| field.display_value().with_unit(&exif).to_string())?;
        let value = value.trim().trim_matches('"').trim().to_owned();
        (!value.is_empty()).then(|| MediaExifField {
            tag: label.to_owned(),
            value,
        })
    })
    .collect()
}

fn ffprobe_metadata(path: &Path) -> Option<FfprobeMediaMetadata> {
    let mut child = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let start = std::time::Instant::now();
    loop {
        if child.try_wait().ok()??.success() {
            let output = child.wait_with_output().ok()?;
            return parse_ffprobe_json(&output.stdout);
        }
        if start.elapsed() >= FFMPEG_PROBE_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn parse_ffprobe_json(bytes: &[u8]) -> Option<FfprobeMediaMetadata> {
    let document: FfprobeDocument = serde_json::from_slice(bytes).ok()?;
    let stream = document.streams.into_iter().next();
    let width = stream.as_ref().and_then(|stream| stream.width);
    let height = stream.as_ref().and_then(|stream| stream.height);
    let duration = document
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .or_else(|| {
            stream
                .as_ref()
                .and_then(|stream| stream.duration.as_deref())
        })
        .and_then(parse_duration_ms);
    let codec = stream.and_then(|stream| stream.codec_name);
    let tags = document.format.and_then(|format| format.tags);
    Some(FfprobeMediaMetadata {
        width,
        height,
        duration_ms: duration,
        codec,
        title: tags.as_ref().and_then(|tags| tags.title.clone()),
        artist: tags.and_then(|tags| tags.artist),
    })
}

fn parse_duration_ms(value: &str) -> Option<u64> {
    let seconds = value.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round().min(u64::MAX as f64) as u64)
}

#[derive(Debug, Clone)]
struct FfprobeMediaMetadata {
    width: Option<u32>,
    height: Option<u32>,
    duration_ms: Option<u64>,
    codec: Option<String>,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FfprobeDocument {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_name: Option<String>,
    duration: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
    tags: Option<FfprobeTags>,
}

#[derive(Debug, Deserialize)]
struct FfprobeTags {
    title: Option<String>,
    artist: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffprobe_json_parser_reads_duration_codec_and_tags() {
        let json = br#"{
            "streams": [{"codec_name": "h264", "duration": "4.500000", "width": 1920, "height": 1080}],
            "format": {
                "duration": "5.250000",
                "tags": {"title": "Clip", "artist": "Camera"}
            }
        }"#;

        let metadata = parse_ffprobe_json(json).expect("metadata");

        assert_eq!(metadata.width, Some(1920));
        assert_eq!(metadata.height, Some(1080));
        assert_eq!(metadata.duration_ms, Some(5250));
        assert_eq!(metadata.codec.as_deref(), Some("h264"));
        assert_eq!(metadata.title.as_deref(), Some("Clip"));
        assert_eq!(metadata.artist.as_deref(), Some("Camera"));
    }
}
