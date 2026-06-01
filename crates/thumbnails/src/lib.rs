#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::fs;

const CACHE_FORMAT_EXTENSION: &str = "png";
const THUMBNAILER_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbnailKey(String);

impl ThumbnailKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailSourceMetadata {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl From<&file_core::EntryMetadata> for ThumbnailSourceMetadata {
    fn from(metadata: &file_core::EntryMetadata) -> Self {
        Self {
            len: metadata.len,
            modified: metadata.modified,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailRequest {
    pub source: PathBuf,
    pub metadata: ThumbnailSourceMetadata,
    pub max_edge: u32,
}

impl ThumbnailRequest {
    pub fn new(source: impl AsRef<Path>, metadata: ThumbnailSourceMetadata, max_edge: u32) -> Self {
        Self {
            source: source.as_ref().to_path_buf(),
            metadata,
            max_edge,
        }
    }

    pub fn key(&self) -> ThumbnailKey {
        thumbnail_key(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailOptions {
    pub max_edge: u32,
}

impl Default for ThumbnailOptions {
    fn default() -> Self {
        Self { max_edge: 256 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub source: PathBuf,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedThumbnail {
    pub key: ThumbnailKey,
    pub source: PathBuf,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub cache_hit: bool,
}

#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("unsupported image format for {path:?}")]
    UnsupportedFormat { path: PathBuf },
    #[error("could not create thumbnail cache directory {path:?}: {source}")]
    CreateCacheDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("thumbnail task failed: {0}")]
    Join(String),
    #[error("could not read cached thumbnail {path:?}: {source}")]
    ReadCachedThumbnail {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("could not read image {path:?}: {source}")]
    ReadImage {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("could not write thumbnail {path:?}: {source}")]
    WriteImage {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("could not move thumbnail {from:?} to {to:?}: {source}")]
    RenameThumbnail {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn is_supported_image_path(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(is_supported_image_extension)
}

pub fn is_supported_image_extension(extension: &str) -> bool {
    ["jpg", "jpeg", "png", "webp"]
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
}

pub async fn load_or_generate_image_thumbnail(
    cache_dir: impl AsRef<Path>,
    request: ThumbnailRequest,
) -> Result<CachedThumbnail, ThumbnailError> {
    if !is_supported_image_path(&request.source) {
        return Err(ThumbnailError::UnsupportedFormat {
            path: request.source,
        });
    }

    let key = request.key();
    let output = cache_dir
        .as_ref()
        .join(format!("{}.{}", key.as_str(), CACHE_FORMAT_EXTENSION));

    if fs::metadata(&output).await.is_ok() {
        match cached_thumbnail_dimensions(output.clone()).await {
            Ok((width, height)) => {
                return Ok(CachedThumbnail {
                    key,
                    source: request.source,
                    output,
                    width,
                    height,
                    cache_hit: true,
                });
            }
            Err(_) => {
                let _ = fs::remove_file(&output).await;
            }
        }
    }

    generate_cached_image_thumbnail(request, output, key).await
}

pub async fn generate_image_thumbnail(
    source: impl AsRef<Path>,
    output: impl AsRef<Path>,
    options: ThumbnailOptions,
) -> Result<Thumbnail, ThumbnailError> {
    let source = source.as_ref().to_path_buf();
    let output = output.as_ref().to_path_buf();

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await.map_err(|source| {
            ThumbnailError::CreateCacheDirectory {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }

    tokio::task::spawn_blocking(move || generate_image_thumbnail_blocking(source, output, options))
        .await
        .map_err(|source| ThumbnailError::Join(source.to_string()))?
}

async fn cached_thumbnail_dimensions(path: PathBuf) -> Result<(u32, u32), ThumbnailError> {
    tokio::task::spawn_blocking(move || {
        image::image_dimensions(&path)
            .map_err(|source| ThumbnailError::ReadCachedThumbnail { path, source })
    })
    .await
    .map_err(|source| ThumbnailError::Join(source.to_string()))?
}

async fn generate_cached_image_thumbnail(
    request: ThumbnailRequest,
    output: PathBuf,
    key: ThumbnailKey,
) -> Result<CachedThumbnail, ThumbnailError> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await.map_err(|source| {
            ThumbnailError::CreateCacheDirectory {
                path: parent.to_path_buf(),
                source,
            }
        })?;
    }

    let temporary_output = temporary_output_path(&output);
    tokio::task::spawn_blocking(move || {
        generate_cached_image_thumbnail_blocking(request, temporary_output, output, key)
    })
    .await
    .map_err(|source| ThumbnailError::Join(source.to_string()))?
}

fn generate_cached_image_thumbnail_blocking(
    request: ThumbnailRequest,
    temporary_output: PathBuf,
    output: PathBuf,
    key: ThumbnailKey,
) -> Result<CachedThumbnail, ThumbnailError> {
    let source = request.source;
    let image = image::ImageReader::open(&source)
        .map_err(|source_error| ThumbnailError::ReadImage {
            path: source.clone(),
            source: image::ImageError::IoError(source_error),
        })?
        .decode()
        .map_err(|source_error| ThumbnailError::ReadImage {
            path: source.clone(),
            source: source_error,
        })?;
    let thumbnail = image.thumbnail(request.max_edge, request.max_edge);
    let width = thumbnail.width();
    let height = thumbnail.height();

    thumbnail
        .save_with_format(&temporary_output, image::ImageFormat::Png)
        .map_err(|source_error| ThumbnailError::WriteImage {
            path: temporary_output.clone(),
            source: source_error,
        })?;

    std::fs::rename(&temporary_output, &output).map_err(|source_error| {
        ThumbnailError::RenameThumbnail {
            from: temporary_output.clone(),
            to: output.clone(),
            source: source_error,
        }
    })?;

    Ok(CachedThumbnail {
        key,
        source,
        output,
        width,
        height,
        cache_hit: false,
    })
}

fn generate_image_thumbnail_blocking(
    source: PathBuf,
    output: PathBuf,
    options: ThumbnailOptions,
) -> Result<Thumbnail, ThumbnailError> {
    let image = image::ImageReader::open(&source)
        .map_err(|source_error| ThumbnailError::ReadImage {
            path: source.clone(),
            source: image::ImageError::IoError(source_error),
        })?
        .decode()
        .map_err(|source_error| ThumbnailError::ReadImage {
            path: source.clone(),
            source: source_error,
        })?;
    let thumbnail = image.thumbnail(options.max_edge, options.max_edge);
    let width = thumbnail.width();
    let height = thumbnail.height();

    thumbnail
        .save(&output)
        .map_err(|source_error| ThumbnailError::WriteImage {
            path: output.clone(),
            source: source_error,
        })?;

    Ok(Thumbnail {
        source,
        output,
        width,
        height,
    })
}

fn temporary_output_path(output: &Path) -> PathBuf {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_else(|| "thumbnail".into());
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{stem}.{}.{}.tmp", std::process::id(), unique))
}

fn thumbnail_key(request: &ThumbnailRequest) -> ThumbnailKey {
    let mut hash = 0xcbf29ce484222325u64;
    update_hash(&mut hash, b"file-manager-thumbnail");
    update_hash(&mut hash, &[THUMBNAILER_VERSION]);
    update_hash(&mut hash, request_path_bytes(&request.source));
    update_hash(&mut hash, &request.metadata.len.to_le_bytes());
    update_system_time_hash(&mut hash, request.metadata.modified);
    update_hash(&mut hash, &request.max_edge.to_le_bytes());
    update_hash(&mut hash, CACHE_FORMAT_EXTENSION.as_bytes());
    ThumbnailKey(format!("{hash:016x}"))
}

#[cfg(unix)]
fn request_path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

#[cfg(not(unix))]
fn request_path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn update_hash(hash: &mut u64, bytes: &[u8]) {
    update_hash_inner(hash, bytes);
}

#[cfg(not(unix))]
fn update_hash(hash: &mut u64, bytes: impl AsRef<[u8]>) {
    update_hash_inner(hash, bytes.as_ref());
}

fn update_hash_inner(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= *byte as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn update_system_time_hash(hash: &mut u64, time: Option<SystemTime>) {
    match time {
        Some(time) => match time.duration_since(UNIX_EPOCH) {
            Ok(duration) => {
                update_hash(hash, b"mtime+");
                update_hash(hash, &duration.as_secs().to_le_bytes());
                update_hash(hash, &duration.subsec_nanos().to_le_bytes());
            }
            Err(error) => {
                let duration = error.duration();
                update_hash(hash, b"mtime-");
                update_hash(hash, &duration.as_secs().to_le_bytes());
                update_hash(hash, &duration.subsec_nanos().to_le_bytes());
            }
        },
        None => update_hash(hash, b"mtime-none"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generate_image_thumbnail_writes_small_image() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        let output = dir.path().join("cache/thumb.png");
        let image = image::RgbImage::new(64, 32);
        image.save(&source).unwrap();

        let thumbnail =
            generate_image_thumbnail(&source, &output, ThumbnailOptions { max_edge: 16 })
                .await
                .unwrap();

        assert_eq!(thumbnail.source, source);
        assert_eq!(thumbnail.output, output);
        assert!(thumbnail.width <= 16);
        assert!(thumbnail.height <= 16);
        assert!(thumbnail.output.exists());
    }

    #[tokio::test]
    async fn load_or_generate_image_thumbnail_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        let cache_dir = dir.path().join("cache");
        let image = image::RgbImage::new(64, 32);
        image.save(&source).unwrap();
        let metadata = std::fs::metadata(&source).unwrap();
        let request = ThumbnailRequest::new(
            &source,
            ThumbnailSourceMetadata {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            },
            16,
        );

        let first = load_or_generate_image_thumbnail(&cache_dir, request.clone())
            .await
            .unwrap();
        let second = load_or_generate_image_thumbnail(&cache_dir, request)
            .await
            .unwrap();

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(first.key, second.key);
        assert_eq!(first.output, second.output);
        assert!(second.width <= 16);
        assert!(second.height <= 16);
    }

    #[test]
    fn supported_image_extensions_match_enabled_codecs() {
        assert!(is_supported_image_path("photo.jpg"));
        assert!(is_supported_image_path("photo.jpeg"));
        assert!(is_supported_image_path("photo.png"));
        assert!(is_supported_image_path("photo.webp"));
        assert!(!is_supported_image_path("photo.gif"));
        assert!(!is_supported_image_path("photo.svg"));
    }
}
