use std::path::Path;

use file_core::{DirectoryEntry, FileKind};
use iced::Task;

use super::FileBrowser;
use crate::commands::load_list_directory_summary_command;
use crate::formatting::format_file_size;
use crate::model::{
    BrowserPaneId, BrowserViewMode, ExpandedDirectoryStatus, ListDirectorySizeDisplayMode,
    ListDirectorySummaryLoadRequest, Message,
};
use crate::operation_queue::QueuedFileOperation;
use crate::thumbnail_cache::ColumnViewport;
use crate::virtual_range::{initial_virtual_range, virtual_range_for_viewport};

#[derive(Debug, Clone)]
struct RenderedListDirectory {
    path: std::path::PathBuf,
    loaded_child_count: Option<usize>,
}

impl FileBrowser {
    pub(super) fn toggle_list_directory_size_display_mode(&mut self) -> Task<Message> {
        self.user_config.list_directory_size_display_mode =
            self.user_config.list_directory_size_display_mode.toggled();
        Task::batch([
            self.persist_user_preferences_command(),
            self.schedule_visible_list_directory_summaries(),
        ])
    }

    pub(super) fn schedule_visible_list_directory_summaries(&mut self) -> Task<Message> {
        let commands = self
            .pane_layout
            .visible_pane_ids()
            .into_iter()
            .map(|pane_id| self.schedule_visible_list_directory_summaries_for_pane(pane_id))
            .collect::<Vec<_>>();
        Task::batch(commands)
    }

    pub(super) fn schedule_visible_list_directory_summaries_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
    ) -> Task<Message> {
        self.schedule_visible_list_directory_summary_range_for_pane(pane_id, None)
    }

    pub(super) fn schedule_visible_list_directory_summary_range_for_pane(
        &mut self,
        pane_id: BrowserPaneId,
        viewport_override: Option<ColumnViewport>,
    ) -> Task<Message> {
        let rendered_directories =
            self.rendered_list_directories_for_pane(pane_id, viewport_override);
        if rendered_directories.is_empty() {
            return Task::none();
        }

        let include_recursive_total_size = self
            .user_config
            .list_directory_size_display_mode
            .uses_recursive_total_size();
        let mut commands = Vec::new();
        for directory in rendered_directories {
            if let Some(loaded_child_count) = directory.loaded_child_count {
                self.list_directory_summary_cache
                    .remember_direct_child_count(directory.path.clone(), loaded_child_count);
            }
            if let Some(request) = self
                .list_directory_summary_cache
                .start_request(directory.path, include_recursive_total_size)
            {
                commands.push(load_list_directory_summary_command(request));
            }
        }

        if commands.is_empty() {
            Task::none()
        } else {
            Task::batch(commands)
        }
    }

    pub(super) fn accept_list_directory_summary(
        &mut self,
        request: ListDirectorySummaryLoadRequest,
        outcome: Result<crate::model::ListDirectorySummary, String>,
    ) -> Task<Message> {
        match outcome {
            Ok(summary) => {
                self.list_directory_summary_cache
                    .store_summary(&request, summary);
            }
            Err(_) => {
                self.list_directory_summary_cache.store_failure(&request);
            }
        }
        Task::none()
    }

    pub(super) fn remember_loaded_list_directory_children(
        &mut self,
        path: &Path,
        direct_child_count: usize,
    ) {
        self.list_directory_summary_cache
            .remember_direct_child_count(path.to_path_buf(), direct_child_count);
    }

    pub(super) fn invalidate_list_directory_summary(&mut self, path: &Path) {
        self.list_directory_summary_cache.invalidate_path(path);
    }

    pub(super) fn invalidate_list_directory_summary_chain(&mut self, path: &Path) {
        for ancestor in path.ancestors() {
            self.invalidate_list_directory_summary(ancestor);
        }
    }

    pub(super) fn invalidate_list_directory_summary_subtree_and_ancestor_chain(
        &mut self,
        path: &Path,
    ) {
        self.list_directory_summary_cache
            .invalidate_path_subtree(path);
        if let Some(parent) = path.parent() {
            self.invalidate_list_directory_summary_chain(parent);
        }
    }

    pub(super) fn invalidate_list_directory_summaries_for_pane(&mut self, pane_id: BrowserPaneId) {
        for path in self.list_directory_paths_for_pane(pane_id) {
            self.invalidate_list_directory_summary(&path);
        }
    }

    pub(super) fn invalidate_list_directory_summaries_for_visible_panes(&mut self) {
        for pane_id in self.pane_layout.visible_pane_ids() {
            self.invalidate_list_directory_summaries_for_pane(pane_id);
        }
    }

    pub(super) fn invalidate_list_directory_summaries_for_file_operation(
        &mut self,
        operation: &QueuedFileOperation,
    ) {
        match operation {
            QueuedFileOperation::Rename { path, .. } => {
                self.invalidate_list_directory_summary_subtree_and_ancestor_chain(path);
            }
            QueuedFileOperation::BatchRename { items } => {
                for item in items {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(&item.from);
                }
            }
            QueuedFileOperation::CreateDirectory { .. }
            | QueuedFileOperation::CreateEmptyFile { .. } => {
                if let Some(path) = operation.created_path() {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(&path);
                }
            }
            QueuedFileOperation::Trash { paths }
            | QueuedFileOperation::DeletePermanently { paths } => {
                for path in paths {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(path);
                }
            }
            QueuedFileOperation::Restore { entries } => {
                for entry in entries {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(
                        &entry.original_path,
                    );
                }
            }
            QueuedFileOperation::Copy { transfers, .. } => {
                for transfer in transfers {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(
                        &transfer.target,
                    );
                }
            }
            QueuedFileOperation::Move { transfers, .. } => {
                for transfer in transfers {
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(
                        &transfer.source,
                    );
                    self.invalidate_list_directory_summary_subtree_and_ancestor_chain(
                        &transfer.target,
                    );
                }
            }
            QueuedFileOperation::CreateArchive { target, .. } => {
                self.invalidate_list_directory_summary_subtree_and_ancestor_chain(target);
            }
            QueuedFileOperation::ExtractArchive { request } => {
                self.invalidate_list_directory_summary_subtree_and_ancestor_chain(
                    &request.destination,
                );
            }
            QueuedFileOperation::DeleteTrashEntries { .. }
            | QueuedFileOperation::EmptyTrash
            | QueuedFileOperation::BuildSearchIndex { .. } => {}
        }
    }

    pub(crate) fn list_directory_size_text(&self, entry: &DirectoryEntry) -> String {
        if entry.kind != FileKind::Directory {
            return format_file_size(entry.metadata.len);
        }

        let Some(summary) = self
            .list_directory_summary_cache
            .summary_for_path(&entry.path)
        else {
            return "-".to_owned();
        };

        match self.user_config.list_directory_size_display_mode {
            ListDirectorySizeDisplayMode::ItemCount => {
                list_item_count_text(summary.direct_child_count)
            }
            ListDirectorySizeDisplayMode::RecursiveTotalSize => summary
                .recursive_total_size_bytes
                .map(format_file_size)
                .unwrap_or_else(|| "-".to_owned()),
        }
    }

    fn rendered_list_directories_for_pane(
        &self,
        pane_id: BrowserPaneId,
        viewport_override: Option<ColumnViewport>,
    ) -> Vec<RenderedListDirectory> {
        if !self.pane_layout.visible_pane_ids().contains(&pane_id) {
            return Vec::new();
        }

        let Some(pane) = self.pane_view(pane_id) else {
            return Vec::new();
        };
        if pane.view_mode != BrowserViewMode::List || pane.is_trash_view || pane.is_loading {
            return Vec::new();
        }

        let total_rows =
            crate::visible_entries::visible_entry_count(pane.entries, pane.expanded_directories);
        let range = viewport_override
            .or_else(|| pane.column_viewports.get(pane.current_dir).copied())
            .map(|viewport| {
                virtual_range_for_viewport(
                    total_rows,
                    crate::list_view::LIST_ROW_HEIGHT,
                    viewport.offset_y,
                    viewport.height,
                    crate::list_view::LIST_OVERSCAN_ROWS,
                )
            })
            .unwrap_or_else(|| {
                initial_virtual_range(
                    total_rows,
                    crate::list_view::LIST_ROW_HEIGHT,
                    crate::list_view::LIST_INITIAL_ROWS,
                )
            });

        crate::visible_entries::visible_entries_in_range(
            pane.entries,
            pane.expanded_directories,
            range.start,
            range.end,
        )
        .into_iter()
        .filter_map(|visible_entry| {
            (visible_entry.entry.kind == FileKind::Directory).then(|| RenderedListDirectory {
                path: visible_entry.entry.path.clone(),
                loaded_child_count: pane
                    .expanded_directories
                    .get(&visible_entry.entry.path)
                    .filter(|expanded| matches!(expanded.status, ExpandedDirectoryStatus::Loaded))
                    .map(|expanded| expanded.entries.len()),
            })
        })
        .collect()
    }

    fn list_directory_paths_for_pane(&self, pane_id: BrowserPaneId) -> Vec<std::path::PathBuf> {
        let Some(pane) = self.pane_view(pane_id) else {
            return Vec::new();
        };
        if pane.view_mode != BrowserViewMode::List || pane.is_trash_view {
            return Vec::new();
        }

        let mut paths = Vec::new();
        collect_list_directory_paths(pane.entries, pane.expanded_directories, &mut paths);
        paths
    }
}

fn list_item_count_text(count: usize) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

fn collect_list_directory_paths(
    entries: &[DirectoryEntry],
    expanded_directories: &std::collections::HashMap<
        std::path::PathBuf,
        crate::model::ExpandedDirectory,
    >,
    paths: &mut Vec<std::path::PathBuf>,
) {
    for entry in entries {
        if entry.kind != FileKind::Directory {
            continue;
        }

        paths.push(entry.path.clone());
        if let Some(expanded) = expanded_directories.get(&entry.path) {
            collect_list_directory_paths(&expanded.entries, expanded_directories, paths);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::model::ListDirectorySummary;
    use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};

    fn remember_summary(
        browser: &mut FileBrowser,
        path: &std::path::Path,
        count: usize,
        size: u64,
    ) {
        browser
            .list_directory_summary_cache
            .remember_direct_child_count(path.to_path_buf(), count);
        let request = browser
            .list_directory_summary_cache
            .start_request(path.to_path_buf(), true)
            .expect("recursive request");
        assert!(browser.list_directory_summary_cache.store_summary(
            &request,
            ListDirectorySummary {
                direct_child_count: count,
                recursive_total_size_bytes: Some(size),
            }
        ));
    }

    #[test]
    fn toggling_list_directory_size_display_mode_updates_preference() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        assert_eq!(
            browser.user_config.list_directory_size_display_mode,
            ListDirectorySizeDisplayMode::ItemCount
        );

        drop(browser.toggle_list_directory_size_display_mode());

        assert_eq!(
            browser.user_config.list_directory_size_display_mode,
            ListDirectorySizeDisplayMode::RecursiveTotalSize
        );
    }

    #[test]
    fn invalidating_directory_summary_chain_clears_ancestors_only() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root = std::path::PathBuf::from("/workspace");
        let project = root.join("project");
        let src = project.join("src");
        let unrelated = root.join("notes");

        for (path, count, size) in [
            (project.clone(), 2usize, 2048u64),
            (src.clone(), 3usize, 1024u64),
            (unrelated.clone(), 1usize, 512u64),
        ] {
            remember_summary(&mut browser, &path, count, size);
        }

        browser.invalidate_list_directory_summary_chain(&src);

        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&src)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&project)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&unrelated)
            .is_some());
    }

    #[test]
    fn delete_operation_invalidation_keeps_unrelated_directory_summaries() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root = std::path::PathBuf::from("/workspace");
        let current_dir = root.join("project");
        let deleted_child = current_dir.join("todo.txt");
        let unrelated = root.join("archive");

        remember_summary(&mut browser, &root, 3, 4096);
        remember_summary(&mut browser, &current_dir, 2, 2048);
        remember_summary(&mut browser, &unrelated, 1, 512);

        browser.invalidate_list_directory_summaries_for_file_operation(
            &QueuedFileOperation::DeletePermanently {
                paths: vec![deleted_child],
            },
        );

        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&current_dir)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&root)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&unrelated)
            .is_some());
    }

    #[test]
    fn move_operation_invalidation_clears_source_and_target_chains_only() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let root = std::path::PathBuf::from("/workspace");
        let source_parent = root.join("source");
        let moved_directory = source_parent.join("entry");
        let source_child = moved_directory.join("child");
        let target_dir = root.join("target");
        let unrelated = root.join("notes");

        remember_summary(&mut browser, &source_parent, 2, 2048);
        remember_summary(&mut browser, &moved_directory, 2, 1536);
        remember_summary(&mut browser, &source_child, 1, 512);
        remember_summary(&mut browser, &target_dir, 1, 1024);
        remember_summary(&mut browser, &unrelated, 4, 512);

        browser.invalidate_list_directory_summaries_for_file_operation(
            &QueuedFileOperation::Move {
                transfers: vec![QueuedTransfer::new(
                    moved_directory,
                    target_dir.join("entry"),
                )],
                verification: file_core::FileOperationVerification::default(),
            },
        );

        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&source_parent)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&source_child)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&target_dir)
            .is_none());
        assert!(browser
            .list_directory_summary_cache
            .summary_for_path(&unrelated)
            .is_some());
    }
}
