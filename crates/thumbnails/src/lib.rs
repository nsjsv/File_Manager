#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::fs;

mod svg;

const CACHE_FORMAT_EXTENSION: &str = "png";
const THUMBNAILER_VERSION: u8 = 3;
const VIDEO_THUMBNAIL_SEEK_TIME: &str = "00:00:01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThumbnailMediaKind {
    Image,
    Video,
}

impl ThumbnailMediaKind {
    fn cache_tag(self) -> &'static [u8] {
        match self {
            Self::Image => b"image",
            Self::Video => b"video",
        }
    }
}

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
        let media_kind =
            thumbnail_media_kind_for_path(&self.source).unwrap_or(ThumbnailMediaKind::Image);
        thumbnail_key(self, media_kind)
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
    #[error("unsupported thumbnail format for {path:?}")]
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
    #[error("could not read SVG image {path:?}: {source}")]
    ReadSvg {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse SVG image {path:?}: {source}")]
    ParseSvg {
        path: PathBuf,
        #[source]
        source: resvg::usvg::Error,
    },
    #[error("could not render SVG image {path:?}")]
    RenderSvg { path: PathBuf },
    #[error("could not write SVG thumbnail {path:?}: {source}")]
    WriteSvgThumbnail {
        path: PathBuf,
        #[source]
        source: std::io::Error,
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
    #[error("could not find ffmpegthumbnailer or ffmpeg to generate video thumbnail for {path:?}")]
    VideoThumbnailerUnavailable { path: PathBuf },
    #[error("could not run video thumbnailer {command} for {path:?}: {source}")]
    RunVideoThumbnailer {
        path: PathBuf,
        command: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("video thumbnailer {command} failed for {path:?}: {message}")]
    GenerateVideoThumbnail {
        path: PathBuf,
        command: &'static str,
        message: String,
    },
}

pub fn is_supported_image_path(path: impl AsRef<Path>) -> bool {
    file_core::is_supported_image_path(path)
}

pub fn is_supported_image_extension(extension: &str) -> bool {
    file_core::is_supported_image_extension(extension)
}

pub fn is_supported_video_path(path: impl AsRef<Path>) -> bool {
    file_core::is_supported_video_path(path)
}

pub fn is_supported_video_extension(extension: &str) -> bool {
    file_core::is_supported_video_extension(extension)
}

pub fn is_supported_thumbnail_path(path: impl AsRef<Path>) -> bool {
    thumbnail_media_kind_for_path(path.as_ref()).is_some()
}

pub async fn load_or_generate_thumbnail(
    cache_dir: impl AsRef<Path>,
    request: ThumbnailRequest,
) -> Result<CachedThumbnail, ThumbnailError> {
    let media_kind = thumbnail_media_kind_for_path(&request.source).ok_or_else(|| {
        ThumbnailError::UnsupportedFormat {
            path: request.source.clone(),
        }
    })?;
    load_or_generate_thumbnail_with_kind(cache_dir, request, media_kind).await
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
    load_or_generate_thumbnail_with_kind(cache_dir, request, ThumbnailMediaKind::Image).await
}

async fn load_or_generate_thumbnail_with_kind(
    cache_dir: impl AsRef<Path>,
    request: ThumbnailRequest,
    media_kind: ThumbnailMediaKind,
) -> Result<CachedThumbnail, ThumbnailError> {
    let started_at = Instant::now();
    let key = thumbnail_key(&request, media_kind);
    let output = cache_dir
        .as_ref()
        .join(format!("{}.{}", key.as_str(), CACHE_FORMAT_EXTENSION));
    tracing::debug!(
        target: "thumbnails",
        source = ?request.source,
        output = ?output,
        key = key.as_str(),
        media_kind = ?media_kind,
        max_edge = request.max_edge,
        "thumbnail requested"
    );

    match cached_thumbnail_dimensions(output.clone()).await {
        Ok((width, height)) => {
            tracing::debug!(
                target: "thumbnails",
                source = ?request.source,
                output = ?output,
                key = key.as_str(),
                max_edge = request.max_edge,
                width,
                height,
                elapsed_ms = started_at.elapsed().as_millis(),
                "thumbnail cache hit"
            );
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

    let generated = generate_cached_thumbnail(request, output, key, media_kind).await;
    match &generated {
        Ok(thumbnail) => tracing::info!(
            target: "thumbnails",
            source = ?thumbnail.source,
            output = ?thumbnail.output,
            key = thumbnail.key.as_str(),
            width = thumbnail.width,
            height = thumbnail.height,
            elapsed_ms = started_at.elapsed().as_millis(),
            "thumbnail generated"
        ),
        Err(error) => tracing::warn!(
            target: "thumbnails",
            error = %error,
            elapsed_ms = started_at.elapsed().as_millis(),
            "thumbnail generation failed"
        ),
    }
    generated
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

    tracing::debug!(
        target: "thumbnails",
        source = ?source,
        output = ?output,
        max_edge = options.max_edge,
        "generate image thumbnail requested"
    );

    tokio::task::spawn_blocking(move || generate_image_thumbnail_blocking(source, output, options))
        .await
        .map_err(|source| ThumbnailError::Join(source.to_string()))?
}

pub async fn load_image_dimensions(source: impl AsRef<Path>) -> Result<(u32, u32), ThumbnailError> {
    let source = source.as_ref().to_path_buf();
    let log_source = source.clone();
    let started_at = Instant::now();
    tracing::debug!(
        target: "thumbnails",
        source = ?log_source,
        "image dimensions requested"
    );
    let dimensions = tokio::task::spawn_blocking(move || load_image_dimensions_blocking(source))
        .await
        .map_err(|source| ThumbnailError::Join(source.to_string()))?;
    match &dimensions {
        Ok((width, height)) => tracing::debug!(
            target: "thumbnails",
            source = ?log_source,
            width,
            height,
            elapsed_ms = started_at.elapsed().as_millis(),
            "image dimensions loaded"
        ),
        Err(error) => tracing::warn!(
            target: "thumbnails",
            source = ?log_source,
            error = %error,
            elapsed_ms = started_at.elapsed().as_millis(),
            "image dimensions failed"
        ),
    }
    dimensions
}

fn load_image_dimensions_blocking(source: PathBuf) -> Result<(u32, u32), ThumbnailError> {
    if is_svg_path(&source) {
        return svg::load_svg_dimensions(&source);
    }

    image::image_dimensions(&source).map_err(|source_error| ThumbnailError::ReadImage {
        path: source,
        source: source_error,
    })
}

async fn cached_thumbnail_dimensions(path: PathBuf) -> Result<(u32, u32), ThumbnailError> {
    tokio::task::spawn_blocking(move || {
        image::image_dimensions(&path)
            .map_err(|source| ThumbnailError::ReadCachedThumbnail { path, source })
    })
    .await
    .map_err(|source| ThumbnailError::Join(source.to_string()))?
}

async fn generate_cached_thumbnail(
    request: ThumbnailRequest,
    output: PathBuf,
    key: ThumbnailKey,
    media_kind: ThumbnailMediaKind,
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
    // 图片解码和外部视频缩略图命令都会阻塞，必须留在 blocking 线程池。
    tokio::task::spawn_blocking(move || match media_kind {
        ThumbnailMediaKind::Image => {
            generate_cached_image_thumbnail_blocking(request, temporary_output, output, key)
        }
        ThumbnailMediaKind::Video => {
            generate_cached_video_thumbnail_blocking(request, temporary_output, output, key)
        }
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
    if is_svg_path(&source) {
        let started_at = Instant::now();
        tracing::debug!(
            target: "thumbnails",
            source = ?source,
            output = ?temporary_output,
            max_edge = request.max_edge,
            "SVG thumbnail render started"
        );
        let (width, height) =
            svg::render_svg_thumbnail(&source, &temporary_output, request.max_edge)?;
        tracing::debug!(
            target: "thumbnails",
            source = ?source,
            output = ?temporary_output,
            width,
            height,
            elapsed_ms = started_at.elapsed().as_millis(),
            "SVG thumbnail render finished"
        );
        return finish_cached_thumbnail(key, source, temporary_output, output, width, height);
    }

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

    finish_cached_thumbnail(key, source, temporary_output, output, width, height)
}

fn generate_cached_video_thumbnail_blocking(
    request: ThumbnailRequest,
    temporary_output: PathBuf,
    output: PathBuf,
    key: ThumbnailKey,
) -> Result<CachedThumbnail, ThumbnailError> {
    let source = request.source;
    run_video_thumbnailer(&source, &temporary_output, request.max_edge)?;
    let (width, height) = image::image_dimensions(&temporary_output).map_err(|source_error| {
        ThumbnailError::ReadCachedThumbnail {
            path: temporary_output.clone(),
            source: source_error,
        }
    })?;

    finish_cached_thumbnail(key, source, temporary_output, output, width, height)
}

fn finish_cached_thumbnail(
    key: ThumbnailKey,
    source: PathBuf,
    temporary_output: PathBuf,
    output: PathBuf,
    width: u32,
    height: u32,
) -> Result<CachedThumbnail, ThumbnailError> {
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
    if is_svg_path(&source) {
        let started_at = Instant::now();
        tracing::debug!(
            target: "thumbnails",
            source = ?source,
            output = ?output,
            max_edge = options.max_edge,
            "SVG direct thumbnail render started"
        );
        let (width, height) = svg::render_svg_thumbnail(&source, &output, options.max_edge)?;
        tracing::debug!(
            target: "thumbnails",
            source = ?source,
            output = ?output,
            width,
            height,
            elapsed_ms = started_at.elapsed().as_millis(),
            "SVG direct thumbnail render finished"
        );
        return Ok(Thumbnail {
            source,
            output,
            width,
            height,
        });
    }

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

fn run_video_thumbnailer(
    source: &Path,
    output: &Path,
    max_edge: u32,
) -> Result<(), ThumbnailError> {
    match run_ffmpegthumbnailer(source, output, max_edge) {
        Ok(()) => Ok(()),
        Err(first_error) => match run_ffmpeg(source, output, max_edge) {
            Ok(()) => Ok(()),
            Err(ThumbnailError::VideoThumbnailerUnavailable { .. }) => Err(first_error),
            Err(second_error) => Err(second_error),
        },
    }
}

fn run_ffmpegthumbnailer(
    source: &Path,
    output: &Path,
    max_edge: u32,
) -> Result<(), ThumbnailError> {
    let command_output = Command::new("ffmpegthumbnailer")
        .arg("-i")
        .arg(source)
        .arg("-o")
        .arg(output)
        .arg("-s")
        .arg(max_edge.to_string())
        .arg("-t")
        .arg("10%")
        .output();
    handle_video_thumbnailer_output("ffmpegthumbnailer", source, command_output)
}

fn run_ffmpeg(source: &Path, output: &Path, max_edge: u32) -> Result<(), ThumbnailError> {
    let scale_filter = format!("scale={max_edge}:{max_edge}:force_original_aspect_ratio=decrease");
    let command_output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-ss")
        .arg(VIDEO_THUMBNAIL_SEEK_TIME)
        .arg("-i")
        .arg(source)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(scale_filter)
        .arg("-f")
        .arg("image2")
        .arg("-vcodec")
        .arg("png")
        .arg(output)
        .output();
    handle_video_thumbnailer_output("ffmpeg", source, command_output)
}

fn handle_video_thumbnailer_output(
    command: &'static str,
    path: &Path,
    output: std::io::Result<Output>,
) -> Result<(), ThumbnailError> {
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(ThumbnailError::GenerateVideoThumbnail {
            path: path.to_path_buf(),
            command,
            message: video_thumbnailer_failure_message(&output),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(ThumbnailError::VideoThumbnailerUnavailable {
                path: path.to_path_buf(),
            })
        }
        Err(source) => Err(ThumbnailError::RunVideoThumbnailer {
            path: path.to_path_buf(),
            command,
            source,
        }),
    }
}

fn video_thumbnailer_failure_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !stdout.is_empty() {
        return stdout;
    }
    format!("exit status {}", output.status)
}

fn thumbnail_media_kind_for_path(path: &Path) -> Option<ThumbnailMediaKind> {
    let extension = path.extension().and_then(std::ffi::OsStr::to_str)?;
    if is_supported_image_extension(extension) {
        Some(ThumbnailMediaKind::Image)
    } else if is_supported_video_extension(extension) {
        Some(ThumbnailMediaKind::Video)
    } else {
        None
    }
}

fn is_svg_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
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
    parent.join(format!(
        ".{stem}.{}.{}.tmp.{CACHE_FORMAT_EXTENSION}",
        std::process::id(),
        unique
    ))
}

fn thumbnail_key(request: &ThumbnailRequest, media_kind: ThumbnailMediaKind) -> ThumbnailKey {
    let mut hash = 0xcbf29ce484222325u64;
    update_hash(&mut hash, b"file-manager-thumbnail");
    update_hash(&mut hash, &[THUMBNAILER_VERSION]);
    update_hash(&mut hash, media_kind.cache_tag());
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
