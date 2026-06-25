use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    Message, SearchIndexProfileSaveReason, SidebarLocation, SidebarLocationKind,
    StartupIndexCapability, StartupIndexEntrySelection, StartupIndexRootSeed,
    StartupIndexSetupState, StartupIndexTargetMode, StartupIndexTreeEntry,
};
use crate::operation_queue::QueuedFileOperation;

const STARTUP_INDEX_TOGGLE_ROTATION_STEP: f32 = 0.18;
const STARTUP_INDEX_TOGGLE_ROTATION_EPSILON: f32 = 0.001;

impl FileBrowser {
    pub(super) fn refresh_startup_index_setup_choices(&mut self) -> Task<Message> {
        if self.user_config.search_mode != SearchBackendMode::Indexed
            || (self.user_config.search_mode_prompt != SearchModePromptStatus::Completed
                && self.search_mode_prompt.is_some())
        {
            self.startup_index_setup = None;
            return Task::none();
        }

        let (common_roots, custom_root) = startup_index_root_choices(&self.sidebar_locations);
        if let Some(setup) = &mut self.startup_index_setup {
            setup.merge_choices(common_roots, custom_root);
        } else {
            self.startup_index_setup =
                StartupIndexSetupState::from_choices(common_roots, custom_root);
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

    pub(super) fn select_startup_index_capability(
        &mut self,
        capability: StartupIndexCapability,
    ) -> Task<Message> {
        if let Some(setup) = &mut self.startup_index_setup {
            setup.select_capability(capability);
        }
        Task::none()
    }

    pub(super) fn select_startup_index_target_mode(
        &mut self,
        target_mode: StartupIndexTargetMode,
    ) -> Task<Message> {
        let (request_generation, load_paths) = {
            let Some(setup) = &mut self.startup_index_setup else {
                return Task::none();
            };
            let load_paths = setup.select_target_mode(target_mode);
            (setup.directory_load_generation(), load_paths)
        };

        startup_index_directory_children_commands(
            load_paths,
            request_generation,
            self.startup_index_scan_options(),
        )
    }

    pub(super) fn press_startup_index_entry(&mut self, entry_id: usize) -> Task<Message> {
        if let Some(setup) = &mut self.startup_index_setup {
            let visible_entry_ids = startup_index_visible_entry_ids(&setup.entries);
            if self.keyboard_modifiers.shift() {
                setup.select_entry_range(entry_id, &visible_entry_ids);
            } else {
                setup.start_entry_selection_drag(entry_id);
            }
        }
        Task::none()
    }

    pub(super) fn enter_startup_index_entry_during_selection_drag(
        &mut self,
        entry_id: usize,
    ) -> Task<Message> {
        if let Some(setup) = &mut self.startup_index_setup {
            let visible_entry_ids = startup_index_visible_entry_ids(&setup.entries);
            setup.enter_entry_during_selection_drag(entry_id, &visible_entry_ids);
        }
        Task::none()
    }

    pub(super) fn finish_startup_index_entry_selection_drag(&mut self) -> Task<Message> {
        if let Some(setup) = &mut self.startup_index_setup {
            setup.finish_entry_selection_drag();
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
        let Some(setup) = &self.startup_index_setup else {
            return Task::none();
        };
        if !setup.can_accept() {
            return Task::none();
        }
        let setup = self
            .startup_index_setup
            .take()
            .expect("startup index setup was checked above");
        let capability = setup
            .capability
            .expect("startup index setup accept requires selected capability");
        let content_enabled = capability.content_enabled();
        let media_metadata_scope = capability.media_metadata_scope();
        let index_requests = setup.selected_index_requests();
        let profile_roots = index_requests
            .iter()
            .map(|request| request.root.clone())
            .collect::<Vec<_>>();
        self.search_index.profile_roots = profile_roots.clone();
        self.search_index.content_index_enabled = content_enabled;
        self.search_index.media_metadata_scope = media_metadata_scope;
        self.user_config.search_mode = SearchBackendMode::Indexed;
        self.user_config.search_mode_prompt = SearchModePromptStatus::Completed;
        self.user_config.search_index_content_enabled = content_enabled;
        self.user_config.search_index_media_scope = media_metadata_scope;
        self.search_index.reset_path_rule_order_from_current_rules();
        self.search_index.service_generation = self.search_index.service_generation.wrapping_add(1);
        self.search_index.pending_startup_index_builds = index_requests;

        Task::batch([
            self.persist_user_preferences_command(),
            search_index_profile_save_command(
                default_search_index_profile(&self.user_config, profile_roots),
                self.user_config.clone(),
                SearchIndexProfileSaveReason::StartupIndexSetup,
            ),
        ])
    }

    pub(super) fn enqueue_pending_startup_index_builds(&mut self) -> Task<Message> {
        let index_requests = std::mem::take(&mut self.search_index.pending_startup_index_builds);
        if index_requests.is_empty() {
            return Task::none();
        }

        let mut queued_index_task = false;
        for request in index_requests {
            self.search_index
                .indexing_roots
                .insert(request.root.clone());
            self.search_index.root_errors.remove(&request.root);
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
            self.show_operation_queue_temporarily()
        } else {
            Task::none()
        }
    }

    fn startup_index_scan_options(&self) -> ScanOptions {
        let Some(setup) = &self.startup_index_setup else {
            return self.options.clone();
        };
        startup_index_scan_options(&self.options, setup)
    }
}

fn startup_index_root_choices(
    locations: &[SidebarLocation],
) -> (Vec<StartupIndexRootSeed>, Option<StartupIndexRootSeed>) {
    let home = locations
        .iter()
        .find(|location| location.kind == SidebarLocationKind::Home)
        .map(|location| location.path.clone());
    let common_roots = startup_index_common_root_seeds(locations, home.as_deref());
    let custom_root = startup_index_custom_root_seed(locations);
    (common_roots, custom_root)
}

fn startup_index_common_root_seeds(
    locations: &[SidebarLocation],
    home: Option<&Path>,
) -> Vec<StartupIndexRootSeed> {
    let mut seen = HashSet::new();
    let mut roots = Vec::new();
    for location in locations {
        if !startup_index_default_root_kind(location.kind) {
            continue;
        }
        if !seen.insert(location.path.clone()) {
            continue;
        }
        roots.push(StartupIndexRootSeed {
            label: location.label.clone(),
            path: location.path.clone(),
            selection: StartupIndexEntrySelection::Selected,
        });
    }
    if let Some(config_root) = startup_index_config_root_seed(home) {
        if seen.insert(config_root.path.clone()) {
            roots.push(config_root);
        }
    }
    roots
}

fn startup_index_custom_root_seed(locations: &[SidebarLocation]) -> Option<StartupIndexRootSeed> {
    locations
        .iter()
        .find(|location| location.kind == SidebarLocationKind::Home)
        .map(|location| StartupIndexRootSeed {
            label: location.label.clone(),
            path: location.path.clone(),
            selection: StartupIndexEntrySelection::Skipped,
        })
}

fn startup_index_config_root_seed(home: Option<&Path>) -> Option<StartupIndexRootSeed> {
    startup_index_config_root_seed_from_path(home, dirs::config_dir())
}

fn startup_index_config_root_seed_from_path(
    home: Option<&Path>,
    path: Option<PathBuf>,
) -> Option<StartupIndexRootSeed> {
    let home = home?;
    let path = path?;
    if !path.exists() || !path.starts_with(home) {
        return None;
    }
    Some(StartupIndexRootSeed {
        label: "User Config".to_owned(),
        path,
        selection: StartupIndexEntrySelection::Selected,
    })
}

fn startup_index_default_root_kind(kind: SidebarLocationKind) -> bool {
    matches!(
        kind,
        SidebarLocationKind::Desktop
            | SidebarLocationKind::Documents
            | SidebarLocationKind::Downloads
            | SidebarLocationKind::Pictures
            | SidebarLocationKind::Videos
            | SidebarLocationKind::Music
    )
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

fn startup_index_visible_entry_ids(entries: &[StartupIndexTreeEntry]) -> Vec<usize> {
    entries
        .iter()
        .filter(|entry| startup_index_entry_visible(entry, entries))
        .map(|entry| entry.id)
        .collect()
}

fn startup_index_entry_visible(
    entry: &StartupIndexTreeEntry,
    entries: &[StartupIndexTreeEntry],
) -> bool {
    let mut parent = entry.parent;
    while let Some(parent_id) = parent {
        let Some(parent_entry) = entries.get(parent_id) else {
            return false;
        };
        if !(parent_entry.is_expanded || parent_entry.toggle_rotation_progress > 0.0) {
            return false;
        }
        parent = parent_entry.parent;
    }

    true
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_index::MediaMetadataScope;

    use super::*;
    use crate::app::FileBrowser;
    use crate::config::{self, SearchBackendMode, SearchModePromptStatus};

    #[test]
    fn accepting_startup_index_setup_waits_for_profile_save_before_queueing_build() {
        let mut config = config::default_user_config();
        config.search_mode = SearchBackendMode::Indexed;
        config.search_mode_prompt = SearchModePromptStatus::Completed;
        let (mut browser, _) = FileBrowser::new(config);
        browser.search_index.base_dir = PathBuf::from("/tmp/search-index");
        let mut setup = StartupIndexSetupState::from_choices(
            vec![StartupIndexRootSeed {
                label: "Home".to_owned(),
                path: PathBuf::from("/home/user"),
                selection: StartupIndexEntrySelection::Selected,
            }],
            None,
        )
        .expect("startup index setup");
        setup.select_target_mode(StartupIndexTargetMode::Common);
        setup.select_capability(StartupIndexCapability::TextAndImageMetadata);
        browser.startup_index_setup = Some(setup);

        let _task = browser.accept_startup_index_setup();

        assert_eq!(
            browser.search_index.profile_roots,
            vec![PathBuf::from("/home/user")]
        );
        assert!(browser.user_config.search_index_content_enabled);
        assert_eq!(
            browser.user_config.search_index_media_scope,
            MediaMetadataScope::Images
        );
        assert!(browser.search_index.content_index_enabled);
        assert_eq!(
            browser.search_index.media_metadata_scope,
            MediaMetadataScope::Images
        );
        assert!(browser.operation_queue.tasks().is_empty());
        assert_eq!(browser.search_index.pending_startup_index_builds.len(), 1);
        assert_eq!(
            browser.search_index.pending_startup_index_builds[0].root,
            PathBuf::from("/home/user")
        );

        let mut saved_profile =
            file_index::IndexProfile::new("default", vec![PathBuf::from("/home/user")]);
        saved_profile.content.enabled = true;
        saved_profile.media.scope = MediaMetadataScope::Images;
        let _task = browser.accept_search_index_profile_save(
            SearchIndexProfileSaveReason::StartupIndexSetup,
            Ok(saved_profile),
        );

        assert!(browser.search_index.pending_startup_index_builds.is_empty());
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

    #[test]
    fn failed_startup_profile_save_does_not_queue_pending_builds() {
        let mut config = config::default_user_config();
        config.search_mode = SearchBackendMode::Indexed;
        config.search_mode_prompt = SearchModePromptStatus::Completed;
        let (mut browser, _) = FileBrowser::new(config);
        let mut setup = StartupIndexSetupState::from_choices(
            vec![StartupIndexRootSeed {
                label: "Home".to_owned(),
                path: PathBuf::from("/home/user"),
                selection: StartupIndexEntrySelection::Selected,
            }],
            None,
        )
        .expect("startup index setup");
        setup.select_target_mode(StartupIndexTargetMode::Common);
        setup.select_capability(StartupIndexCapability::Text);
        browser.startup_index_setup = Some(setup);

        let _task = browser.accept_startup_index_setup();
        let _task = browser.accept_search_index_profile_save(
            SearchIndexProfileSaveReason::StartupIndexSetup,
            Err("profile save failed".to_owned()),
        );

        assert!(browser.search_index.pending_startup_index_builds.is_empty());
        assert!(browser.search_index.indexing_roots.is_empty());
        assert!(browser.operation_queue.tasks().is_empty());
        assert_eq!(
            browser.search_index.profile_error.as_deref(),
            Some("profile save failed")
        );
    }

    #[test]
    fn startup_index_common_roots_use_common_directories_not_home() {
        let seeds = startup_index_common_root_seeds(
            &[
                SidebarLocation {
                    label: "Home".to_owned(),
                    path: PathBuf::from("/home/user"),
                    kind: SidebarLocationKind::Home,
                },
                SidebarLocation {
                    label: "Documents".to_owned(),
                    path: PathBuf::from("/home/user/Documents"),
                    kind: SidebarLocationKind::Documents,
                },
                SidebarLocation {
                    label: "Downloads".to_owned(),
                    path: PathBuf::from("/home/user/Downloads"),
                    kind: SidebarLocationKind::Downloads,
                },
            ],
            None,
        );

        assert_eq!(
            seeds.iter().map(|seed| &seed.path).collect::<Vec<_>>(),
            vec![
                &PathBuf::from("/home/user/Documents"),
                &PathBuf::from("/home/user/Downloads")
            ]
        );
        assert!(seeds
            .iter()
            .all(|seed| seed.selection == StartupIndexEntrySelection::Selected));
    }

    #[test]
    fn startup_index_config_root_seed_requires_existing_path_inside_home() {
        let temp_dir = tempfile::tempdir().expect("temp home");
        let outside_dir = tempfile::tempdir().expect("outside temp");
        let home = temp_dir.path();
        let config_dir = home.join(".config");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        let outside = outside_dir.path().join(".config");
        std::fs::create_dir_all(&outside).expect("outside config dir");

        let seed = startup_index_config_root_seed_from_path(Some(home), Some(config_dir.clone()))
            .expect("config seed");

        assert_eq!(seed.label, "User Config");
        assert_eq!(seed.path, config_dir);
        assert!(startup_index_config_root_seed_from_path(Some(home), Some(outside)).is_none());
    }
}
