use std::path::{Path, PathBuf};

use file_core::DirectoryEntry;
use iced::Task;
use thumbnails::ThumbnailRequest;

use super::{FileBrowser, PaneIconGridViewport};
use crate::commands::thumbnail_batch_command;
use crate::model::{
    sanitized_application_log_detail, BrowserPane, BrowserPaneId, BrowserViewMode,
    IconGridViewport, Message, PreviewContent, PreviewState,
};
use crate::thumbnail_cache::{
    request_for_entry, request_for_transfer_conflict_path, ColumnViewport, ThumbnailHandleEntry,
    ThumbnailLoadOutcome, ThumbnailLoadPolicy, ThumbnailLoadResult, ThumbnailPriority,
    ThumbnailPurpose, ThumbnailScope, COLUMN_THUMBNAIL_EDGE, LIST_THUMBNAIL_EDGE,
    PREVIEW_THUMBNAIL_MAX_EDGE, TRANSFER_CONFLICT_THUMBNAIL_EDGE,
};
use crate::virtual_range::{initial_virtual_range, virtual_range_for_viewport};

const OVERSCAN_ROWS: usize = 28;
const INITIAL_THUMBNAIL_ROWS: usize = OVERSCAN_ROWS * 2 + 1;
const PREVIEW_THUMBNAIL_MIN_EDGE: u32 = 512;
const PREVIEW_RESIZE_EXTRA_PIXELS: u32 = 128;

impl FileBrowser {
    pub(super) fn schedule_thumbnail_refresh(&mut self) -> Task<Message> {
        self.schedule_interaction_thumbnails();
        self.schedule_rendered_browser_thumbnails();
        self.pump_thumbnail_queue()
    }

    pub(super) fn schedule_thumbnail_refresh_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
    ) -> Task<Message> {
        if pane_id == self.active_pane_id() {
            self.schedule_interaction_thumbnails();
        }
        self.schedule_rendered_browser_thumbnails_for_pane(pane_id);
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
        Task::batch([
            self.schedule_visible_list_directory_summary_range_for_pane(pane_id, Some(viewport)),
            self.pump_thumbnail_queue(),
        ])
    }

    pub(super) fn handle_icon_grid_scrolled(
        &mut self,
        pane_id: BrowserPaneId,
        offset_y: f32,
        width: f32,
        height: f32,
    ) -> Task<Message> {
        let Some(directory) = self.pane_view(pane_id).map(|pane| pane.current_dir.clone()) else {
            return Task::none();
        };
        self.icon_grid_viewports.insert(
            pane_id,
            PaneIconGridViewport {
                directory,
                viewport: IconGridViewport {
                    offset_y: offset_y.max(0.0),
                    width: width.max(1.0),
                    height: height.max(1.0),
                },
            },
        );
        self.schedule_visible_icon_grid_thumbnails_for_pane(pane_id);
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

    pub(super) fn schedule_transfer_conflict_thumbnails(&mut self) -> Task<Message> {
        self.enqueue_current_transfer_conflict_thumbnail_requests();
        self.pump_thumbnail_queue()
    }

    pub(super) fn accept_image_preview_dimensions(
        &mut self,
        path: PathBuf,
        dimensions: Result<(u32, u32), String>,
    ) -> Task<Message> {
        let active_preview_loading = self.is_active_preview_loading(&path);
        if !active_preview_loading {
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
        tracing::debug!(
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
                ThumbnailLoadResult::Ready(thumbnail) => {
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
                    let is_current_request =
                        self.is_current_thumbnail_request(&outcome.work.request);
                    let source = thumbnail.source.clone();
                    let active_preview_loading = self.is_active_preview_loading(&source);
                    if !is_current_request {
                        continue;
                    }
                    let ready = self
                        .thumbnail_cache
                        .insert_ready(thumbnail, outcome.work.request.max_edge);
                    if outcome.work.purpose == ThumbnailPurpose::Preview && active_preview_loading {
                        self.preview = Some(PreviewState::Ready(thumbnail_preview_content(
                            source, ready,
                        )));
                    }
                }
                ThumbnailLoadResult::CacheMiss => {
                    tracing::debug!(
                        target: "app_ui::thumbnail",
                        source = ?outcome.work.request.source,
                        purpose = ?outcome.work.purpose,
                        "cached thumbnail missed"
                    );
                }
                ThumbnailLoadResult::Failed(error) => {
                    let log_error = sanitized_application_log_detail(&error);
                    tracing::debug!(
                        target: "app_ui::thumbnail",
                        source = ?outcome.work.request.source,
                        purpose = ?outcome.work.purpose,
                        error = %log_error,
                        "thumbnail batch item failed"
                    );
                    self.thumbnail_cache.mark_failure(key);
                    let is_current_request =
                        self.is_current_thumbnail_request(&outcome.work.request);
                    let active_preview_loading =
                        self.is_active_preview_loading(&outcome.work.request.source);
                    if outcome.work.purpose == ThumbnailPurpose::Preview
                        && active_preview_loading
                        && is_current_request
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
        if self
            .pane_view(self.active_pane_id())
            .is_some_and(|pane| pane.view_mode == BrowserViewMode::Icons)
        {
            return;
        }

        if let Some(path) = self.selected.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.enqueue_list_thumbnail_for_entry(
                    &entry,
                    self.active_entry_thumbnail_edge(),
                    ThumbnailPriority::Focused,
                );
            }
        }

        if let Some(path) = self.hovered_entry.clone() {
            if let Some(entry) = self.entry_for_path(&path).cloned() {
                self.enqueue_list_thumbnail_for_entry(
                    &entry,
                    self.active_entry_thumbnail_edge(),
                    ThumbnailPriority::Focused,
                );
            }
        }
    }

    fn schedule_rendered_browser_thumbnails(&mut self) {
        for pane_id in self.pane_layout.visible_pane_ids() {
            self.schedule_rendered_browser_thumbnails_for_pane(pane_id);
        }
    }

    fn schedule_rendered_browser_thumbnails_for_pane(&mut self, pane_id: BrowserPaneId) {
        match self.pane_view(pane_id).map(|pane| pane.view_mode) {
            Some(BrowserViewMode::List) => {
                self.schedule_visible_list_thumbnails_for_pane(pane_id);
                return;
            }
            Some(BrowserViewMode::Icons) => {
                self.schedule_visible_icon_grid_thumbnails_for_pane(pane_id);
                return;
            }
            Some(BrowserViewMode::Columns) => {}
            None => return,
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

    fn schedule_visible_icon_grid_thumbnails_for_pane(&mut self, pane_id: BrowserPaneId) {
        let icon_edge = self.user_config.icon_grid_size;
        let thumbnail_edge = crate::icon_grid_geometry::thumbnail_edge(icon_edge);
        let Some((directory, requests)) = self.pane_view(pane_id).map(|pane| {
            let range = crate::icon_grid_geometry::visible_entry_range(
                pane.icon_grid_viewport,
                pane.entries.len(),
                icon_edge,
            );
            let requests = pane.entries[range.start_entry..range.end_entry]
                .iter()
                .filter_map(|entry| request_for_entry(entry, thumbnail_edge))
                .collect::<Vec<_>>();
            (pane.current_dir.clone(), requests)
        }) else {
            return;
        };
        let scope = thumbnail_scope_for_pane_directory(pane_id, &directory);
        let keep = requests
            .iter()
            .map(ThumbnailRequest::key)
            .collect::<Vec<_>>();
        self.thumbnail_cache.prune_scope_except(&scope, &keep);
        for request in requests {
            self.enqueue_list_thumbnail_request_for_scope(
                request,
                ThumbnailPriority::Visible,
                scope.clone(),
            );
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
            self.enqueue_list_thumbnail_request_for_scope(
                request,
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
            self.enqueue_list_thumbnail_request_for_scope(request, priority, scope.clone());
        }
    }

    fn enqueue_list_thumbnail_for_entry(
        &mut self,
        entry: &DirectoryEntry,
        max_edge: u32,
        priority: ThumbnailPriority,
    ) {
        let Some(request) = request_for_entry(entry, max_edge) else {
            return;
        };
        self.enqueue_thumbnail_request(request, ThumbnailPurpose::List, priority);
    }

    fn enqueue_current_transfer_conflict_thumbnail_requests(&mut self) {
        let requests = self
            .transfer_conflict
            .as_ref()
            .and_then(|state| state.current_conflict())
            .map(|conflict| {
                [
                    request_for_transfer_conflict_path(
                        &conflict.target,
                        &conflict.target_metadata,
                        TRANSFER_CONFLICT_THUMBNAIL_EDGE,
                    ),
                    request_for_transfer_conflict_path(
                        &conflict.source,
                        &conflict.source_metadata,
                        TRANSFER_CONFLICT_THUMBNAIL_EDGE,
                    ),
                ]
            });

        let Some(requests) = requests else {
            return;
        };

        for request in requests.into_iter().flatten() {
            self.enqueue_thumbnail_request(
                request,
                ThumbnailPurpose::TransferConflict,
                ThumbnailPriority::Focused,
            );
        }
    }

    fn enqueue_thumbnail_request(
        &mut self,
        request: ThumbnailRequest,
        purpose: ThumbnailPurpose,
        priority: ThumbnailPriority,
    ) {
        match self.thumbnail_load_policy_for_path(&request.source) {
            ThumbnailLoadPolicy::LoadOrGenerate => {
                self.thumbnail_cache
                    .enqueue_request(request, purpose, priority);
            }
            ThumbnailLoadPolicy::CacheOnly => {
                self.thumbnail_cache
                    .enqueue_cached_request(request, purpose, priority);
            }
        }
    }

    fn enqueue_list_thumbnail_request_for_scope(
        &mut self,
        request: ThumbnailRequest,
        priority: ThumbnailPriority,
        scope: ThumbnailScope,
    ) {
        match self.thumbnail_load_policy_for_path(&request.source) {
            ThumbnailLoadPolicy::LoadOrGenerate => {
                self.thumbnail_cache.enqueue_request_for_scope(
                    request,
                    ThumbnailPurpose::List,
                    priority,
                    scope,
                );
            }
            ThumbnailLoadPolicy::CacheOnly => {
                self.thumbnail_cache.enqueue_cached_request_for_scope(
                    request,
                    ThumbnailPurpose::List,
                    priority,
                    scope,
                );
            }
        }
    }

    fn thumbnail_load_policy_for_path(&self, path: &Path) -> ThumbnailLoadPolicy {
        if self.path_is_mounted_network(path) && !self.network_list_thumbnail_downloads_enabled() {
            ThumbnailLoadPolicy::CacheOnly
        } else {
            ThumbnailLoadPolicy::LoadOrGenerate
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
            crate::three_column_view::COLUMN_ENTRY_SCROLL_HEIGHT,
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
            || self
                .transfer_conflict
                .as_ref()
                .is_some_and(|state| thumbnail_request_matches_transfer_conflict(state, request))
    }

    fn is_active_preview_loading(&self, path: &Path) -> bool {
        matches!(
            &self.preview,
            Some(PreviewState::Loading(loading_path)) if loading_path == path
        )
    }

    fn active_entry_thumbnail_edge(&self) -> u32 {
        match self
            .pane_view(self.active_pane_id())
            .map(|pane| pane.view_mode)
        {
            Some(BrowserViewMode::Columns) => COLUMN_THUMBNAIL_EDGE,
            Some(BrowserViewMode::Icons) => {
                crate::icon_grid_geometry::thumbnail_edge(self.user_config.icon_grid_size)
            }
            Some(BrowserViewMode::List) | None => LIST_THUMBNAIL_EDGE,
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

fn thumbnail_request_matches_transfer_conflict(
    state: &crate::model::TransferConflictState,
    request: &ThumbnailRequest,
) -> bool {
    state.current_conflict().is_some_and(|conflict| {
        thumbnail_request_matches_transfer_conflict_path(
            &conflict.target,
            &conflict.target_metadata,
            request,
        ) || thumbnail_request_matches_transfer_conflict_path(
            &conflict.source,
            &conflict.source_metadata,
            request,
        )
    })
}

fn thumbnail_request_matches_transfer_conflict_path(
    path: &Path,
    metadata: &file_core::TransferConflictMetadata,
    request: &ThumbnailRequest,
) -> bool {
    request_for_transfer_conflict_path(path, metadata, request.max_edge)
        .is_some_and(|current| current.key() == request.key())
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
mod tests;

fn thumbnail_preview_content(path: PathBuf, ready: ThumbnailHandleEntry) -> PreviewContent {
    PreviewContent::Image {
        path,
        handle: ready.handle,
        width: ready.width,
        height: ready.height,
        max_edge: ready.max_edge,
    }
}
