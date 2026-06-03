use std::path::{Path, PathBuf};

use file_core::DirectoryEntry;
use iced::Command;
use thumbnails::ThumbnailRequest;

use super::FileBrowser;
use crate::commands::thumbnail_batch_command;
use crate::model::{Message, PreviewContent, PreviewState};
use crate::thumbnail_cache::{
    request_for_entry, ColumnViewport, ThumbnailHandleEntry, ThumbnailLoadOutcome,
    ThumbnailPriority, ThumbnailPurpose, LIST_THUMBNAIL_EDGE, PREVIEW_THUMBNAIL_MAX_EDGE,
};

const ESTIMATED_ROW_HEIGHT: f32 = 56.0;
const INITIAL_PREFETCH_ROWS: usize = 96;
const OVERSCAN_ROWS: usize = 28;
const PREVIEW_THUMBNAIL_MIN_EDGE: u32 = 512;
const PREVIEW_RESIZE_EXTRA_PIXELS: u32 = 128;

impl FileBrowser {
    pub(super) fn schedule_thumbnail_refresh(&mut self) -> Command<Message> {
        self.schedule_interaction_thumbnails();
        self.schedule_rendered_column_thumbnails();
        self.pump_thumbnail_queue()
    }

    pub(super) fn handle_column_scrolled(
        &mut self,
        directory: PathBuf,
        offset_y: f32,
        height: f32,
    ) -> Command<Message> {
        self.column_viewports.insert(
            directory.clone(),
            ColumnViewport {
                offset_y: offset_y.max(0.0),
                height: height.max(1.0),
            },
        );
        self.schedule_directory_thumbnails(&directory, ThumbnailPriority::Focused);
        self.pump_thumbnail_queue()
    }

    pub(super) fn request_preview_thumbnail_for_entry(
        &mut self,
        entry: DirectoryEntry,
    ) -> Command<Message> {
        let max_edge = self.preview_thumbnail_edge();
        let Some(request) = request_for_entry(&entry, max_edge) else {
            return Command::none();
        };

        if let Some(ready) = self.thumbnail_cache.ready_for_request(&request).cloned() {
            self.preview = Some(PreviewState::Ready(thumbnail_preview_content(
                entry.path, ready,
            )));
            return Command::none();
        }

        self.thumbnail_cache.enqueue_request(
            request,
            ThumbnailPurpose::Preview,
            ThumbnailPriority::Preview,
        );
        self.pump_thumbnail_queue()
    }

    pub(super) fn refresh_preview_thumbnail_for_size(&mut self) -> Command<Message> {
        let Some((path, max_edge)) = self.preview.as_ref().and_then(|preview| match preview {
            PreviewState::Ready(PreviewContent::Image { path, max_edge, .. }) => {
                Some((path.clone(), *max_edge))
            }
            _ => None,
        }) else {
            return Command::none();
        };
        let desired_edge = self.preview_thumbnail_edge();
        if desired_edge <= max_edge + PREVIEW_RESIZE_EXTRA_PIXELS {
            return Command::none();
        }

        let Some(entry) = self.entry_for_path(&path).cloned() else {
            return Command::none();
        };
        self.preview = Some(PreviewState::Loading(path));
        self.request_preview_thumbnail_for_entry(entry)
    }

    pub(super) fn accept_image_preview_dimensions(
        &mut self,
        path: PathBuf,
        dimensions: Result<(u32, u32), String>,
    ) -> Command<Message> {
        if !self.is_active_preview_loading(&path) {
            return Command::none();
        }
        let (width, height) = match dimensions {
            Ok((width, height)) if width > 0 && height > 0 => (width, height),
            Ok(_) => {
                self.preview = Some(PreviewState::Error(
                    "Image preview has invalid dimensions".to_owned(),
                ));
                return self.open_image_preview_error_window();
            }
            Err(error) => {
                self.preview = Some(PreviewState::Error(error));
                return self.open_image_preview_error_window();
            }
        };

        let Some(entry) = self.entry_for_path(&path).cloned() else {
            self.preview = Some(PreviewState::Error(
                "Selected item is no longer available".to_owned(),
            ));
            return self.open_image_preview_error_window();
        };

        Command::batch([
            self.open_image_preview_window_for_dimensions(width, height),
            self.request_preview_thumbnail_for_entry(entry),
        ])
    }

    pub(super) fn accept_thumbnail_batch(
        &mut self,
        outcomes: Vec<ThumbnailLoadOutcome>,
    ) -> Command<Message> {
        let mut commands = Vec::new();
        for outcome in outcomes {
            let key = outcome.work.key();
            self.thumbnail_cache.finish(&key);

            match outcome.result {
                Ok(thumbnail) => {
                    if !self.is_current_thumbnail_request(&outcome.work.request) {
                        continue;
                    }
                    let source = thumbnail.source.clone();
                    let ready = self
                        .thumbnail_cache
                        .insert_ready(thumbnail, outcome.work.request.max_edge);
                    if outcome.work.purpose == ThumbnailPurpose::Preview
                        && self.is_active_preview_loading(&source)
                    {
                        self.preview = Some(PreviewState::Ready(thumbnail_preview_content(
                            source, ready,
                        )));
                    }
                }
                Err(error) => {
                    self.thumbnail_cache.mark_failure(key);
                    if outcome.work.purpose == ThumbnailPurpose::Preview
                        && self.is_active_preview_loading(&outcome.work.request.source)
                        && self.is_current_thumbnail_request(&outcome.work.request)
                    {
                        self.preview = Some(PreviewState::Error(error));
                    }
                }
            }
        }

        self.schedule_interaction_thumbnails();
        commands.push(self.pump_thumbnail_queue());
        Command::batch(commands)
    }

    pub(super) fn pump_thumbnail_queue(&mut self) -> Command<Message> {
        let works = self.thumbnail_cache.take_next_batch();
        if works.is_empty() {
            return Command::none();
        }
        thumbnail_batch_command(self.thumbnail_cache.cache_dir(), works)
    }

    fn schedule_interaction_thumbnails(&mut self) {
        if let Some(path) = self.selected.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.thumbnail_cache.enqueue_entry(
                    &entry,
                    LIST_THUMBNAIL_EDGE,
                    ThumbnailPurpose::List,
                    ThumbnailPriority::Focused,
                );
            }
        }

        if let Some(path) = self.hovered_entry.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.thumbnail_cache.enqueue_entry(
                    &entry,
                    LIST_THUMBNAIL_EDGE,
                    ThumbnailPurpose::List,
                    ThumbnailPriority::Focused,
                );
            }
        }
    }

    fn schedule_rendered_column_thumbnails(&mut self) {
        let directories = rendered_column_directories_for_browser(self);
        let focused_index = directories.len().saturating_sub(1);
        for (index, directory) in directories.iter().enumerate() {
            let priority = if index == focused_index {
                ThumbnailPriority::Focused
            } else {
                ThumbnailPriority::Visible
            };
            self.schedule_directory_thumbnails(directory, priority);
        }
    }

    fn schedule_directory_thumbnails(&mut self, directory: &Path, priority: ThumbnailPriority) {
        let requests = self.thumbnail_requests_for_directory_range(directory);
        for request in requests {
            self.thumbnail_cache
                .enqueue_request(request, ThumbnailPurpose::List, priority);
        }
    }

    fn thumbnail_requests_for_directory_range(&self, directory: &Path) -> Vec<ThumbnailRequest> {
        let Some(entries) = self.entries_for_directory(directory) else {
            return Vec::new();
        };
        if entries.is_empty() {
            return Vec::new();
        }

        let (start, end) = self.thumbnail_range_for_directory(directory, entries.len());
        entries[start..end]
            .iter()
            .filter_map(|entry| request_for_entry(entry, LIST_THUMBNAIL_EDGE))
            .collect()
    }

    fn entries_for_directory(&self, directory: &Path) -> Option<&[DirectoryEntry]> {
        if directory == self.current_dir {
            return Some(&self.entries);
        }

        self.expanded_directories
            .get(directory)
            .map(|expanded| expanded.entries.as_slice())
    }

    fn thumbnail_range_for_directory(&self, directory: &Path, len: usize) -> (usize, usize) {
        let Some(viewport) = self.column_viewports.get(directory).copied() else {
            return (0, len.min(INITIAL_PREFETCH_ROWS));
        };
        let first_visible = (viewport.offset_y / ESTIMATED_ROW_HEIGHT).floor().max(0.0) as usize;
        let visible_count = (viewport.height / ESTIMATED_ROW_HEIGHT).ceil().max(1.0) as usize;
        let start = first_visible.saturating_sub(OVERSCAN_ROWS);
        let end = first_visible
            .saturating_add(visible_count)
            .saturating_add(OVERSCAN_ROWS * 2)
            .min(len);
        (start, end)
    }

    fn preview_thumbnail_edge(&self) -> u32 {
        self.preview_size
            .width
            .max(self.preview_size.height)
            .ceil()
            .max(PREVIEW_THUMBNAIL_MIN_EDGE as f32)
            .min(PREVIEW_THUMBNAIL_MAX_EDGE as f32) as u32
    }

    fn is_current_thumbnail_request(&self, request: &ThumbnailRequest) -> bool {
        self.entry_for_path(&request.source)
            .and_then(|entry| request_for_entry(entry, request.max_edge))
            .is_some_and(|current| current.key() == request.key())
    }

    fn is_active_preview_loading(&self, path: &Path) -> bool {
        matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == path
        )
    }
}

fn rendered_column_directories_for_browser(browser: &FileBrowser) -> Vec<PathBuf> {
    crate::three_column_view::column_directories(browser)
}

fn thumbnail_preview_content(path: PathBuf, ready: ThumbnailHandleEntry) -> PreviewContent {
    PreviewContent::Image {
        path,
        handle: ready.handle,
        width: ready.width,
        height: ready.height,
        max_edge: ready.max_edge,
    }
}
