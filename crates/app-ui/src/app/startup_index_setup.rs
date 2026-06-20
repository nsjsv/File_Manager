use std::collections::HashSet;
use std::path::PathBuf;

use file_core::{DirectoryEntry, ScanOptions};
use file_index::FileSearchIndexMode;
use iced::Task;

use super::FileBrowser;
use crate::commands::{
    default_search_index_profile, search_index_profile_save_command,
    startup_index_directory_children_command,
};
use crate::config::{SearchBackendMode, SearchModePromptStatus};
use crate::model::{
    Message, SidebarLocation, SidebarLocationKind, StartupIndexEntrySelection,
    StartupIndexRootSeed, StartupIndexSetupState, StartupIndexTreeEntry,
};
use crate::operation_queue::QueuedFileOperation;

const STARTUP_INDEX_TOGGLE_ROTATION_STEP: f32 = 0.18;
const STARTUP_INDEX_TOGGLE_ROTATION_EPSILON: f32 = 0.001;

impl FileBrowser {
    pub(super) fn refresh_startup_index_setup_choices(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed
            || self.user_config.search_mode_prompt != SearchModePromptStatus::Completed
        {
            self.startup_index_setup = None;
            return Task::none();
        }

        let roots = startup_index_root_seeds(&self.sidebar_locations);
        if let Some(setup) = &mut self.startup_index_setup {
            setup.merge_roots(roots);
        } else {
            self.startup_index_setup = StartupIndexSetupState::from_roots(roots);
        }
        self.load_expanded_startup_index_roots()
    }

    fn load_expanded_startup_index_roots(&mut self) -> Task<Message> {
        let (request_generation, load_paths) = {
            let Some(setup) = &mut self.startup_index_setup else {
                return Task::none();
            };
            (
                setup.directory_load_generation(),
                setup.expand_roots_waiting_for_children(),
            )
        };

        startup_index_directory_children_commands(
            load_paths,
            request_generation,
            self.startup_index_scan_options(),
        )
    }

    pub(super) fn toggle_startup_index_hidden_content_visibility(&mut self) -> Task<Message> {
        let (request_generation, load_paths) = {
            let Some(setup) = &mut self.startup_index_setup else {
                return Task::none();
            };
            let load_paths = setup.toggle_hidden_content_visibility();
            (setup.directory_load_generation(), load_paths)
        };

        startup_index_directory_children_commands(
            load_paths,
            request_generation,
            self.startup_index_scan_options(),
        )
    }

    pub(super) fn toggle_startup_index_entry(&mut self, entry_id: usize) -> Task<Message> {
        if let Some(setup) = &mut self.startup_index_setup {
            setup.toggle_entry_selection(entry_id);
        }
        Task::none()
    }

    pub(super) fn toggle_startup_index_directory(&mut self, entry_id: usize) -> Task<Message> {
        let options = self.startup_index_scan_options();
        let (request_generation, path) = {
            let Some(setup) = &mut self.startup_index_setup else {
                return Task::none();
            };
            let Some(path) = setup.toggle_directory(entry_id) else {
                return Task::none();
            };
            (setup.directory_load_generation(), path)
        };

        startup_index_directory_children_command(path, request_generation, options)
    }

    pub(super) fn startup_index_tree_animation_is_active(&self) -> bool {
        self.startup_index_setup
            .as_ref()
            .is_some_and(|setup| setup.entries.iter().any(startup_index_rotation_is_active))
    }

    pub(super) fn advance_startup_index_tree_animation(&mut self) -> Task<Message> {
        let Some(setup) = &mut self.startup_index_setup else {
            return Task::none();
        };

        for entry in setup
            .entries
            .iter_mut()
            .filter(|entry| entry.is_directory())
        {
            let target = startup_index_rotation_target(entry);
            if entry.toggle_rotation_progress < target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    + STARTUP_INDEX_TOGGLE_ROTATION_STEP)
                    .min(target);
            } else if entry.toggle_rotation_progress > target {
                entry.toggle_rotation_progress = (entry.toggle_rotation_progress
                    - STARTUP_INDEX_TOGGLE_ROTATION_STEP)
                    .max(target);
            }
        }

        Task::none()
    }

    pub(super) fn accept_startup_index_directory_children(
        &mut self,
        request_generation: u64,
        parent_path: PathBuf,
        children_outcome: Result<Vec<DirectoryEntry>, String>,
    ) -> Task<Message> {
        let Some(setup) = &mut self.startup_index_setup else {
            return Task::none();
        };
        if request_generation != setup.directory_load_generation() {
            return Task::none();
        }
        match children_outcome {
            Ok(children) => setup.accept_directory_children(&parent_path, children),
            Err(error) => setup.accept_directory_error(&parent_path, error),
        }
        Task::none()
    }

    pub(super) fn accept_startup_index_setup(&mut self) -> Task<Message> {
        let Some(setup) = self.startup_index_setup.take() else {
            return Task::none();
        };
        let index_requests = setup.selected_index_requests();
        let profile_roots = index_requests
            .iter()
            .map(|request| request.root.clone())
            .collect::<Vec<_>>();
        self.search_index.profile_roots = profile_roots.clone();
        self.search_index.reset_path_rule_order_from_current_rules();
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);

        let mut tasks = vec![
            self.persist_user_config_command(),
            search_index_profile_save_command(
                default_search_index_profile(&self.user_config, profile_roots),
                self.user_config.clone(),
            ),
        ];
        let mut queued_index_task = false;
        for request in index_requests {
            self.search_index
                .indexing_roots
                .insert(request.root.clone());
            self.search_index.errors.remove(&request.root);
            if let Some(error) =
                self.operation_queue
                    .enqueue(QueuedFileOperation::BuildSearchIndex {
                        profile_id: self.search_index.profile_id.clone(),
                        root: request.root,
                        index_base_dir: self.search_index.base_dir.clone(),
                        selected_paths: request.selected_paths,
                        mode: FileSearchIndexMode::FullRebuild,
                    })
            {
                self.error = Some(error);
            }
            queued_index_task = true;
        }
        if queued_index_task {
            tasks.push(self.show_operation_queue_temporarily());
        }
        Task::batch(tasks)
    }

    pub(super) fn skip_startup_index_setup(&mut self) -> Task<Message> {
        self.startup_index_setup = None;
        self.persist_user_config_command()
    }

    fn startup_index_scan_options(&self) -> ScanOptions {
        let Some(setup) = &self.startup_index_setup else {
            return self.options.clone();
        };
        startup_index_scan_options(&self.options, setup)
    }
}

fn startup_index_root_seeds(locations: &[SidebarLocation]) -> Vec<StartupIndexRootSeed> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for location in locations {
        if location.kind != SidebarLocationKind::Home {
            continue;
        }
        if !seen.insert(location.path.clone()) {
            continue;
        }
        roots.push(StartupIndexRootSeed {
            label: location.label.clone(),
            path: location.path.clone(),
            selection: StartupIndexEntrySelection::Skipped,
        });
    }
    roots
}

fn startup_index_directory_children_commands(
    paths: Vec<PathBuf>,
    request_generation: u64,
    options: ScanOptions,
) -> Task<Message> {
    let tasks = paths
        .into_iter()
        .map(|path| {
            startup_index_directory_children_command(path, request_generation, options.clone())
        })
        .collect::<Vec<_>>();

    Task::batch(tasks)
}

fn startup_index_scan_options(
    base_options: &ScanOptions,
    setup: &StartupIndexSetupState,
) -> ScanOptions {
    let mut options = base_options.clone();
    options.include_hidden = setup.show_hidden_entries;
    options
}

fn startup_index_rotation_is_active(entry: &StartupIndexTreeEntry) -> bool {
    entry.is_directory()
        && (entry.toggle_rotation_progress - startup_index_rotation_target(entry)).abs()
            > STARTUP_INDEX_TOGGLE_ROTATION_EPSILON
}

fn startup_index_rotation_target(entry: &StartupIndexTreeEntry) -> f32 {
    if entry.is_expanded {
        1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::app::FileBrowser;
    use crate::config::{self, SearchBackendMode, SearchModePromptStatus};

    #[test]
    fn accepting_startup_index_setup_saves_profile_and_queues_build() {
        let mut config = config::default_user_config();
        config.search_mode = SearchBackendMode::Indexed;
        config.search_mode_prompt = SearchModePromptStatus::Completed;
        let (mut browser, _) = FileBrowser::new(config);
        browser.search_index.base_dir = PathBuf::from("/tmp/search-index");
        browser.startup_index_setup =
            StartupIndexSetupState::from_roots(vec![StartupIndexRootSeed {
                label: "Home".to_owned(),
                path: PathBuf::from("/home/user"),
                selection: StartupIndexEntrySelection::Selected,
            }]);

        let _task = browser.accept_startup_index_setup();

        assert_eq!(
            browser.search_index.profile_roots,
            vec![PathBuf::from("/home/user")]
        );
        let queued = browser.operation_queue.tasks();
        assert_eq!(queued.len(), 1);
        match &queued[0].operation {
            QueuedFileOperation::BuildSearchIndex {
                profile_id,
                root,
                index_base_dir,
                selected_paths,
                mode,
            } => {
                assert_eq!(profile_id, "default");
                assert_eq!(root, &PathBuf::from("/home/user"));
                assert_eq!(index_base_dir, &PathBuf::from("/tmp/search-index"));
                assert_eq!(selected_paths, &vec![PathBuf::from("/home/user")]);
                assert_eq!(*mode, FileSearchIndexMode::FullRebuild);
            }
            operation => panic!("expected search index build task, got {operation:?}"),
        }
    }
}
