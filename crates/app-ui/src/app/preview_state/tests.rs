use std::path::{Path, PathBuf};

use file_core::{DirectoryEntry, EntryMetadata, FileKind};
use iced::widget::image;

use crate::app::FileBrowser;
use crate::config::ui_thread_startup_config;
use crate::model::{ImagePreviewContent, PreviewContent, PreviewState};

fn browser_with_image(path: &Path) -> FileBrowser {
    let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
    let mut metadata = EntryMetadata::default();
    metadata.len = 1;
    browser.entries = vec![DirectoryEntry::new(
        path.to_path_buf(),
        FileKind::File,
        metadata,
        false,
        false,
        false,
    )]
    .into();
    browser
}

fn raster_result() -> crate::original_image_preview::OriginalImagePreview {
    crate::original_image_preview::OriginalImagePreview::Raster {
        raster_handle: image::Handle::from_rgba(4, 3, vec![0; 48]),
        placeholder_handle: image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
        width: 4,
        height: 3,
    }
}

fn thumbnail_content(path: PathBuf, handle: image::Handle) -> PreviewContent {
    PreviewContent::Image(ImagePreviewContent::Thumbnail {
        path,
        handle,
        width: 1,
        height: 1,
        max_edge: 512,
    })
}

#[test]
fn original_result_replaces_loading_preview_with_raster_content() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Loading(path.clone()));
    let generation = browser.next_original_image_preview_generation();

    let command =
        browser.accept_original_image_preview(path.clone(), generation, Ok(raster_result()));

    assert!(matches!(
        browser.preview,
        Some(PreviewState::Ready(PreviewContent::Image(
            ImagePreviewContent::OriginalRaster {
                width: 4,
                height: 3,
                ..
            }
        )))
    ));
    assert!(browser.preview_window.is_some());
    assert!(command.units() > 0);
}

#[test]
fn original_result_replaces_thumbnail_placeholder() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    let thumbnail_handle = image::Handle::from_rgba(1, 1, vec![1, 2, 3, 255]);
    let thumbnail_id = thumbnail_handle.id();
    browser.preview = Some(PreviewState::Ready(thumbnail_content(
        path.clone(),
        thumbnail_handle,
    )));
    let preview_window = iced::window::Id::unique();
    browser.preview_window = Some(preview_window);
    let generation = browser.next_original_image_preview_generation();

    let command =
        browser.accept_original_image_preview(path.clone(), generation, Ok(raster_result()));

    assert!(matches!(
        browser.preview,
        Some(PreviewState::Ready(PreviewContent::Image(
            ImagePreviewContent::OriginalRaster {
                placeholder_handle,
                ..
            }
        ))) if placeholder_handle.id() == thumbnail_id
    ));
    assert_eq!(browser.preview_window, Some(preview_window));
    assert_eq!(command.units(), 0);
}

#[test]
fn stale_original_result_is_rejected_after_preview_clear() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Loading(path.clone()));
    let stale_generation = browser.next_original_image_preview_generation();
    browser.clear_preview();
    browser.preview = Some(PreviewState::Loading(path.clone()));

    drop(browser.accept_original_image_preview(path, stale_generation, Ok(raster_result())));

    assert!(matches!(browser.preview, Some(PreviewState::Loading(_))));
}

#[test]
fn original_failure_preserves_path_for_retry() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Loading(path.clone()));
    let generation = browser.next_original_image_preview_generation();

    let command = browser.accept_original_image_preview(
        path.clone(),
        generation,
        Err("could not decode original image".to_owned()),
    );

    assert!(matches!(
        &browser.preview,
        Some(PreviewState::ImageError {
            path: current,
            error: message,
        }) if current == &path && message.contains("decode")
    ));
    assert!(browser.preview_window.is_some());
    assert!(command.units() > 0);

    drop(browser.retry_image_preview(path.clone()));
    assert!(matches!(
        browser.preview,
        Some(PreviewState::Loading(current)) if current == path
    ));
}

#[test]
fn original_result_cannot_replace_existing_original_content() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Ready(PreviewContent::Image(
        ImagePreviewContent::OriginalRaster {
            raster_handle: image::Handle::from_rgba(2, 2, vec![255; 16]),
            placeholder_handle: image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
            width: 2,
            height: 2,
        },
    )));
    let generation = browser.next_original_image_preview_generation();

    drop(browser.accept_original_image_preview(path, generation, Ok(raster_result())));

    assert!(matches!(
        browser.preview,
        Some(PreviewState::Ready(PreviewContent::Image(
            ImagePreviewContent::OriginalRaster {
                width: 2,
                height: 2,
                ..
            }
        )))
    ));
}

#[test]
fn clear_preview_drops_original_image_state() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Ready(PreviewContent::Image(
        ImagePreviewContent::OriginalRaster {
            raster_handle: image::Handle::from_rgba(16, 16, vec![7; 1024]),
            placeholder_handle: image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
            width: 16,
            height: 16,
        },
    )));

    browser.clear_preview();

    assert!(browser.preview.is_none());
}

#[test]
fn clear_preview_drops_pending_original_image_request() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Loading(path.clone()));
    let generation = browser.next_original_image_preview_generation();
    drop(browser.accept_image_preview_dimensions(path, generation, Ok((320, 240))));
    assert!(browser.pending_original_image_preview.is_some());
    let cancellation = browser
        .original_image_preview_cancel
        .clone()
        .expect("pending original image request cancellation");

    browser.clear_preview();

    assert!(browser.pending_original_image_preview.is_none());
    assert!(cancellation.is_cancelled());
    assert!(browser.original_image_preview_cancel.is_none());
}

#[test]
fn new_original_request_drops_previous_pending_request() {
    let path = PathBuf::from("/workspace/photo.png");
    let mut browser = browser_with_image(&path);
    browser.preview = Some(PreviewState::Loading(path.clone()));
    let generation = browser.next_original_image_preview_generation();
    drop(browser.accept_image_preview_dimensions(path, generation, Ok((320, 240))));
    assert!(browser.pending_original_image_preview.is_some());
    let previous_cancellation = browser
        .original_image_preview_cancel
        .clone()
        .expect("pending original image request cancellation");

    browser.next_original_image_preview_generation();

    assert!(browser.pending_original_image_preview.is_none());
    assert!(previous_cancellation.is_cancelled());
    assert!(browser
        .original_image_preview_cancel
        .as_ref()
        .is_some_and(|cancellation| !cancellation.is_cancelled()));
}
