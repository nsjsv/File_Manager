use std::path::{Path, PathBuf};

use file_core::DirectoryEntry;
use iced::Task;
use thumbnails::ThumbnailRequest;

use super::FileBrowser;
use crate::commands::thumbnail_batch_command;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserViewMode, Message, PreviewContent, PreviewState,
};
use crate::thumbnail_cache::{
    request_for_entry, ColumnViewport, ThumbnailHandleEntry, ThumbnailLoadOutcome,
    ThumbnailPriority, ThumbnailPurpose, ThumbnailScope, COLUMN_THUMBNAIL_EDGE,
    LIST_THUMBNAIL_EDGE, PREVIEW_THUMBNAIL_MAX_EDGE,
};
use crate::virtual_range::{initial_virtual_range, virtual_range_for_viewport};

const OVERSCAN_ROWS: usize = 28;
const INITIAL_THUMBNAIL_ROWS: usize = OVERSCAN_ROWS * 2 + 1;
const PREVIEW_THUMBNAIL_MIN_EDGE: u32 = 512;
const PREVIEW_RESIZE_EXTRA_PIXELS: u32 = 128;

impl FileBrowser {
    pub(super) fn schedule_thumbnail_refresh(&mut self) -> Task<Message> {
        self.schedule_interaction_thumbnails();
        self.schedule_rendered_column_thumbnails();
        self.pump_thumbnail_queue()
    }

    pub(super) fn schedule_thumbnail_refresh_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
    ) -> Task<Message> {
        if pane_id == self.active_pane_id() {
            self.schedule_interaction_thumbnails();
        }
        self.schedule_rendered_column_thumbnails_for_pane(pane_id);
        self.pump_thumbnail_queue()
    }

    pub(super) fn handle_column_scrolled(
        &mut self,
        pane_id: BrowserPaneId,
        directory: PathBuf,
        offset_y: f32,
        height: f32,
    ) -> Task<Message> {
        let viewport = ColumnViewport {
            offset_y: offset_y.max(0.0),
            height: height.max(1.0),
        };
        if pane_id == self.active_pane_id() {
            self.column_viewports.insert(directory.clone(), viewport);
        } else {
            let Some(pane) = self.pane_by_id_mut(pane_id) else {
                return Task::none();
            };
            pane.column_viewports.insert(directory.clone(), viewport);
        }
        self.schedule_column_directory_thumbnails_for_pane(
            pane_id,
            &directory,
            ThumbnailPriority::Focused,
        );
        self.pump_thumbnail_queue()
    }

    pub(super) fn handle_list_scrolled(
        &mut self,
        pane_id: BrowserPaneId,
        offset_y: f32,
        height: f32,
    ) -> Task<Message> {
        let viewport = ColumnViewport {
            offset_y: offset_y.max(0.0),
            height: height.max(1.0),
        };
        let Some(directory) = self.pane_view(pane_id).map(|pane| pane.current_dir.clone()) else {
            return Task::none();
        };
        if pane_id == self.active_pane_id() {
            self.column_viewports.insert(directory, viewport);
        } else if let Some(pane) = self.pane_by_id_mut(pane_id) {
            pane.column_viewports.insert(directory, viewport);
        }
        self.schedule_visible_list_thumbnail_range_for_pane(pane_id, Some(viewport));
        self.pump_thumbnail_queue()
    }

    pub(super) fn request_preview_thumbnail_for_entry(
        &mut self,
        entry: DirectoryEntry,
    ) -> Task<Message> {
        let max_edge = self.preview_thumbnail_edge();
        let Some(request) = request_for_entry(&entry, max_edge) else {
            tracing::debug!(
                target: "app_ui::preview",
                path = ?entry.path,
                max_edge,
                "preview thumbnail request skipped"
            );
            return Task::none();
        };

        if let Some(ready) = self.thumbnail_cache.ready_for_request(&request).cloned() {
            tracing::debug!(
                target: "app_ui::preview",
                path = ?entry.path,
                max_edge,
                "preview thumbnail ready from cache"
            );
            self.preview = Some(PreviewState::Ready(thumbnail_preview_content(
                entry.path, ready,
            )));
            return Task::none();
        }

        tracing::debug!(
            target: "app_ui::preview",
            path = ?entry.path,
            max_edge,
            "preview thumbnail queued"
        );
        self.thumbnail_cache.enqueue_request(
            request,
            ThumbnailPurpose::Preview,
            ThumbnailPriority::Preview,
        );
        self.pump_thumbnail_queue()
    }

    pub(super) fn refresh_preview_thumbnail_for_size(&mut self) -> Task<Message> {
        let Some((path, max_edge)) = self.preview.as_ref().and_then(|preview| match preview {
            PreviewState::Ready(PreviewContent::Image { path, max_edge, .. }) => {
                Some((path.clone(), *max_edge))
            }
            _ => None,
        }) else {
            return Task::none();
        };
        let desired_edge = self.preview_thumbnail_edge();
        if desired_edge <= max_edge + PREVIEW_RESIZE_EXTRA_PIXELS {
            tracing::debug!(
                target: "app_ui::preview",
                path = ?path,
                current_max_edge = max_edge,
                desired_edge,
                "preview thumbnail refresh skipped"
            );
            return Task::none();
        }

        let Some(entry) = self.entry_for_path(&path).cloned() else {
            return Task::none();
        };
        self.preview = Some(PreviewState::Loading(path));
        self.request_preview_thumbnail_for_entry(entry)
    }

    pub(super) fn accept_image_preview_dimensions(
        &mut self,
        path: PathBuf,
        dimensions: Result<(u32, u32), String>,
    ) -> Task<Message> {
        if !self.is_active_preview_loading(&path) {
            return Task::none();
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
        tracing::info!(
            target: "app_ui::preview",
            path = ?path,
            width,
            height,
            "image preview dimensions accepted"
        );

        let Some(entry) = self.entry_for_path(&path).cloned() else {
            self.preview = Some(PreviewState::Error(
                "Selected item is no longer available".to_owned(),
            ));
            return self.open_image_preview_error_window();
        };

        Task::batch([
            self.open_image_preview_window_for_dimensions(width, height),
            self.request_preview_thumbnail_for_entry(entry),
        ])
    }

    pub(super) fn accept_thumbnail_batch(
        &mut self,
        outcomes: Vec<ThumbnailLoadOutcome>,
    ) -> Task<Message> {
        let mut commands = Vec::new();
        for outcome in outcomes {
            let key = outcome.work.key();
            self.thumbnail_cache.finish(&key);

            match outcome.result {
                Ok(thumbnail) => {
                    tracing::debug!(
                        target: "app_ui::thumbnail",
                        source = ?thumbnail.source,
                        output = ?thumbnail.output,
                        purpose = ?outcome.work.purpose,
                        width = thumbnail.width,
                        height = thumbnail.height,
                        cache_hit = thumbnail.cache_hit,
                        "thumbnail batch item loaded"
                    );
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
                    tracing::warn!(
                        target: "app_ui::thumbnail",
                        source = ?outcome.work.request.source,
                        purpose = ?outcome.work.purpose,
                        error = %error,
                        "thumbnail batch item failed"
                    );
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
        Task::batch(commands)
    }

    pub(super) fn pump_thumbnail_queue(&mut self) -> Task<Message> {
        let works = self.thumbnail_cache.take_next_batch();
        if works.is_empty() {
            return Task::none();
        }
        tracing::debug!(
            target: "app_ui::thumbnail",
            count = works.len(),
            "thumbnail batch scheduled"
        );
        thumbnail_batch_command(self.thumbnail_cache.cache_dir(), works)
    }

    fn schedule_interaction_thumbnails(&mut self) {
        if let Some(path) = self.selected.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.thumbnail_cache.enqueue_entry(
                    &entry,
                    self.active_entry_thumbnail_edge(),
                    ThumbnailPurpose::List,
                    ThumbnailPriority::Focused,
                );
            }
        }

        if let Some(path) = self.hovered_entry.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.thumbnail_cache.enqueue_entry(
                    &entry,
                    self.active_entry_thumbnail_edge(),
                    ThumbnailPurpose::List,
                    ThumbnailPriority::Focused,
                );
            }
        }
    }

    fn schedule_rendered_column_thumbnails(&mut self) {
        for pane_id in self.pane_layout.visible_pane_ids() {
            self.schedule_rendered_column_thumbnails_for_pane(pane_id);
        }
    }

    fn schedule_rendered_column_thumbnails_for_pane(&mut self, pane_id: BrowserPaneId) {
        if self
            .pane_view(pane_id)
            .is_some_and(|pane| pane.view_mode == BrowserViewMode::List)
        {
            self.schedule_visible_list_thumbnails_for_pane(pane_id);
            return;
        }

        let directories = rendered_column_directories_for_browser(self, pane_id);
        let focused_index = directories.len().saturating_sub(1);
        for (index, directory) in directories.iter().enumerate() {
            let priority = if index == focused_index {
                ThumbnailPriority::Focused
            } else {
                ThumbnailPriority::Visible
            };
            self.schedule_column_directory_thumbnails_for_pane(pane_id, directory, priority);
        }
    }

    fn schedule_visible_list_thumbnails_for_pane(&mut self, pane_id: BrowserPaneId) {
        self.schedule_visible_list_thumbnail_range_for_pane(pane_id, None);
    }

    fn schedule_visible_list_thumbnail_range_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
        viewport: Option<ColumnViewport>,
    ) {
        let Some(pane) = self.pane_view(pane_id) else {
            return;
        };
        let directory = pane.current_dir.clone();
        let total_rows =
            crate::visible_entries::visible_entry_count(pane.entries, pane.expanded_directories);
        let (start, end) =
            thumbnail_range_for_row_height(viewport, total_rows, crate::list_view::LIST_ROW_HEIGHT);
        let entries = crate::visible_entries::visible_entries_in_range(
            pane.entries,
            pane.expanded_directories,
            start,
            end,
        );
        let scope = thumbnail_scope_for_pane_directory(pane_id, &directory);
        let requests = entries
            .iter()
            .filter_map(|visible_entry| request_for_entry(visible_entry.entry, LIST_THUMBNAIL_EDGE))
            .collect::<Vec<_>>();
        let keep = requests
            .iter()
            .map(ThumbnailRequest::key)
            .collect::<Vec<_>>();
        self.thumbnail_cache.prune_scope_except(&scope, &keep);
        for request in requests {
            self.thumbnail_cache.enqueue_request_for_scope(
                request,
                ThumbnailPurpose::List,
                ThumbnailPriority::Visible,
                scope.clone(),
            );
        }
    }

    fn schedule_column_directory_thumbnails_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
        directory: &Path,
        priority: ThumbnailPriority,
    ) {
        let requests = self.thumbnail_requests_for_pane_directory_range(pane_id, directory);
        let scope = thumbnail_scope_for_pane_directory(pane_id, directory);
        let keep = requests
            .iter()
            .map(ThumbnailRequest::key)
            .collect::<Vec<_>>();
        self.thumbnail_cache.prune_scope_except(&scope, &keep);
        for request in requests {
            self.thumbnail_cache.enqueue_request_for_scope(
                request,
                ThumbnailPurpose::List,
                priority,
                scope.clone(),
            );
        }
    }

    fn thumbnail_requests_for_pane_directory_range(
        &self,
        pane_id: BrowserPaneId,
        directory: &Path,
    ) -> Vec<ThumbnailRequest> {
        let Some(entries) = self.entries_for_pane_directory(pane_id, directory) else {
            return Vec::new();
        };
        if entries.is_empty() {
            return Vec::new();
        }

        let (start, end) =
            self.thumbnail_range_for_pane_directory(pane_id, directory, entries.len());
        entries[start..end]
            .iter()
            .filter_map(|entry| request_for_entry(entry, COLUMN_THUMBNAIL_EDGE))
            .collect()
    }

    fn entries_for_pane_directory(
        &self,
        pane_id: BrowserPaneId,
        directory: &Path,
    ) -> Option<&[DirectoryEntry]> {
        if pane_id == self.active_pane_id() {
            return self.entries_for_directory(directory);
        }

        let pane = self.pane_by_id(pane_id)?;
        if directory == pane.current_dir {
            return Some(&pane.entries);
        }

        pane.expanded_directories
            .get(directory)
            .map(|expanded| expanded.entries.as_slice())
    }

    fn entries_for_directory(&self, directory: &Path) -> Option<&[DirectoryEntry]> {
        if directory == self.current_dir {
            return Some(&self.entries);
        }

        self.expanded_directories
            .get(directory)
            .map(|expanded| expanded.entries.as_slice())
    }

    fn thumbnail_range_for_pane_directory(
        &self,
        pane_id: BrowserPaneId,
        directory: &Path,
        len: usize,
    ) -> (usize, usize) {
        thumbnail_range_for_row_height(
            self.column_viewport_for_pane_directory(pane_id, directory),
            len,
            crate::three_column_view::COLUMN_ENTRY_HEIGHT,
        )
    }

    fn column_viewport_for_pane_directory(
        &self,
        pane_id: BrowserPaneId,
        directory: &Path,
    ) -> Option<ColumnViewport> {
        if pane_id == self.active_pane_id() {
            return self.column_viewports.get(directory).copied();
        }

        self.pane_by_id(pane_id)
            .and_then(|pane| pane.column_viewports.get(directory).copied())
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
            .is_some_and(|entry| thumbnail_request_matches_entry(entry, request))
            || self
                .panes
                .iter()
                .any(|pane| thumbnail_request_matches_pane(pane, request))
    }

    fn is_active_preview_loading(&self, path: &Path) -> bool {
        matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == path
        )
    }

    fn active_entry_thumbnail_edge(&self) -> u32 {
        if self
            .pane_view(self.active_pane_id())
            .is_some_and(|pane| pane.view_mode == BrowserViewMode::Columns)
        {
            COLUMN_THUMBNAIL_EDGE
        } else {
            LIST_THUMBNAIL_EDGE
        }
    }
}

fn rendered_column_directories_for_browser(
    browser: &FileBrowser,
    pane_id: BrowserPaneId,
) -> Vec<PathBuf> {
    browser
        .pane_view(pane_id)
        .map(crate::three_column_view::column_directories_for_pane)
        .unwrap_or_default()
}

fn thumbnail_request_matches_entry(entry: &DirectoryEntry, request: &ThumbnailRequest) -> bool {
    request_for_entry(entry, request.max_edge).is_some_and(|current| current.key() == request.key())
}

fn thumbnail_request_matches_pane(pane: &BrowserPane, request: &ThumbnailRequest) -> bool {
    pane.entries
        .iter()
        .chain(
            pane.expanded_directories
                .values()
                .flat_map(|expanded| expanded.entries.iter()),
        )
        .find(|entry| entry.path == request.source)
        .is_some_and(|entry| thumbnail_request_matches_entry(entry, request))
}

fn thumbnail_range_for_row_height(
    viewport: Option<ColumnViewport>,
    len: usize,
    row_height: f32,
) -> (usize, usize) {
    let range = viewport
        .map(|viewport| {
            virtual_range_for_viewport(
                len,
                row_height,
                viewport.offset_y,
                viewport.height,
                OVERSCAN_ROWS,
            )
        })
        .unwrap_or_else(|| initial_virtual_range(len, row_height, INITIAL_THUMBNAIL_ROWS));
    (range.start, range.end)
}

fn thumbnail_scope_for_pane_directory(pane_id: BrowserPaneId, directory: &Path) -> ThumbnailScope {
    ThumbnailScope::PaneDirectory {
        pane_id: pane_id.key(),
        directory: directory.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use file_core::{EntryMetadata, FileKind};

    use super::*;
    use crate::animated_image_preview::{
        AnimatedImageFrame, AnimatedImagePlayback, AnimatedImagePreview,
    };
    use crate::config::ui_thread_startup_config;
    use crate::model::{BrowserPaneLayout, BrowserTab, SplitAxis};

    #[test]
    fn missing_viewport_schedules_initial_thumbnail_rows() {
        let range = thumbnail_range_for_row_height(None, 100, crate::list_view::LIST_ROW_HEIGHT);

        assert_eq!(range, (0, INITIAL_THUMBNAIL_ROWS));
    }

    #[test]
    fn measured_viewport_schedules_visible_rows_with_overscan() {
        let viewport = ColumnViewport {
            offset_y: crate::list_view::LIST_ROW_HEIGHT * 40.0,
            height: crate::list_view::LIST_ROW_HEIGHT * 3.0,
        };

        let range =
            thumbnail_range_for_row_height(Some(viewport), 120, crate::list_view::LIST_ROW_HEIGHT);

        assert_eq!(range, (12, 71));
    }

    #[test]
    fn column_thumbnail_range_uses_column_row_height() {
        let viewport = ColumnViewport {
            offset_y: crate::three_column_view::COLUMN_ENTRY_HEIGHT * 40.0,
            height: crate::three_column_view::COLUMN_ENTRY_HEIGHT * 3.0,
        };

        let range = thumbnail_range_for_row_height(
            Some(viewport),
            120,
            crate::three_column_view::COLUMN_ENTRY_HEIGHT,
        );

        assert_eq!(range, (12, 71));
    }

    #[test]
    fn inactive_pane_thumbnail_request_matches_current_entry() {
        let (browser, _, _, image_entry) = browser_with_inactive_image_pane();
        let request = request_for_entry(&image_entry, LIST_THUMBNAIL_EDGE).expect("image request");

        assert!(browser.is_current_thumbnail_request(&request));
    }

    #[test]
    fn inactive_pane_thumbnail_range_uses_its_own_viewport() {
        let (browser, inactive_id, inactive_dir, image_entry) = browser_with_inactive_image_pane();

        let requests = browser
            .thumbnail_requests_for_pane_directory_range(inactive_id, inactive_dir.as_path());

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source, image_entry.path);
    }

    #[test]
    fn inactive_pane_thumbnail_range_schedules_svg_request() {
        let (browser, inactive_id, inactive_dir, image_entry) =
            browser_with_inactive_pane_image("/inactive/vector.svg");

        let requests = browser
            .thumbnail_requests_for_pane_directory_range(inactive_id, inactive_dir.as_path());

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source, image_entry.path);
    }

    #[test]
    fn list_scrolled_schedules_visible_list_thumbnail_requests() {
        let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
        browser.view_mode = BrowserViewMode::List;
        browser.entries = vec![
            image_entry("/workspace/photo-0.png"),
            image_entry("/workspace/vector.svg"),
            image_entry("/workspace/photo-2.png"),
        ];

        browser.schedule_visible_list_thumbnail_range_for_pane(
            BrowserPaneId::PRIMARY,
            Some(ColumnViewport {
                offset_y: 0.0,
                height: crate::list_view::LIST_ROW_HEIGHT * 3.0,
            }),
        );

        let queued_sources = browser
            .thumbnail_cache
            .take_next_batch()
            .into_iter()
            .map(|work| work.request.source.clone())
            .collect::<HashSet<_>>();

        assert!(queued_sources.contains(&PathBuf::from("/workspace/photo-0.png")));
        assert!(queued_sources.contains(&PathBuf::from("/workspace/vector.svg")));
    }

    #[test]
    fn preview_thumbnail_refresh_skips_same_edge_window_resize() {
        let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
        let image_entry = image_entry("/workspace/vector.svg");
        browser.entries = vec![image_entry.clone()];
        browser.preview_size = crate::model::PreviewSize {
            width: 640.0,
            height: 480.0,
        };
        browser.preview = Some(PreviewState::Ready(PreviewContent::Image {
            path: image_entry.path.clone(),
            handle: iced::widget::image::Handle::from_path("/tmp/vector-thumb.png"),
            width: 320,
            height: 240,
            max_edge: 640,
        }));

        let command = browser.refresh_preview_thumbnail_for_size();

        assert_eq!(command.units(), 0);
    }

    #[test]
    fn preview_thumbnail_refresh_skips_animated_image_preview() {
        let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
        browser.preview_size = crate::model::PreviewSize {
            width: 1400.0,
            height: 1000.0,
        };
        let animated_path = PathBuf::from("/workspace/loop.gif");
        let first_frame = AnimatedImageFrame {
            path: animated_path.clone(),
            generation: 1,
            position: std::time::Duration::ZERO,
            handle: iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
            width: 1,
            height: 1,
        };
        browser.preview = Some(PreviewState::Ready(PreviewContent::AnimatedImage(
            AnimatedImagePreview::new(
                animated_path,
                first_frame,
                1,
                Some(std::time::Duration::from_millis(40)),
                AnimatedImagePlayback::Animated,
            )
            .expect("animated image preview"),
        )));

        let command = browser.refresh_preview_thumbnail_for_size();

        assert_eq!(command.units(), 0);
    }

    fn browser_with_inactive_image_pane() -> (FileBrowser, BrowserPaneId, PathBuf, DirectoryEntry) {
        browser_with_inactive_pane_image("/inactive/photo.png")
    }

    fn browser_with_inactive_pane_image(
        image_path: &str,
    ) -> (FileBrowser, BrowserPaneId, PathBuf, DirectoryEntry) {
        let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
        let inactive_id = BrowserPaneId(1);
        let inactive_dir = PathBuf::from("/inactive");
        let image_entry = image_entry(image_path);
        let tab = BrowserTab::directory(1, inactive_dir.clone());

        browser.panes.push(BrowserPane {
            id: inactive_id,
            current_dir: inactive_dir.clone(),
            is_trash_view: false,
            entries: vec![image_entry.clone()],
            directory_loading_placeholder_entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            deepest_open_column_directory: None,
            expanded_directories: HashMap::new(),
            view_mode: crate::model::BrowserViewMode::Columns,
            column_viewports: HashMap::from([(
                inactive_dir.clone(),
                ColumnViewport {
                    offset_y: 0.0,
                    height: crate::list_view::LIST_ROW_HEIGHT,
                },
            )]),
            tabs: vec![tab.clone()],
            active_tab_id: tab.id,
            path_input: inactive_dir.to_string_lossy().into_owned(),
            path_suggestions: Vec::new(),
            path_suggestion_selection: None,
            path_suggestion_generation: 0,
            directory_load_generation: 0,
            directory_load_cancel: None,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            is_loading: false,
        });
        browser.pane_layout = BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            first: BrowserPaneId::PRIMARY,
            second: inactive_id,
            active: BrowserPaneId::PRIMARY,
        };

        (browser, inactive_id, inactive_dir, image_entry)
    }

    fn image_entry(path: &str) -> DirectoryEntry {
        DirectoryEntry::new(
            PathBuf::from(path),
            FileKind::File,
            EntryMetadata {
                len: 10,
                modified: None,
                readonly: false,
            },
            false,
            false,
            false,
        )
    }
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
