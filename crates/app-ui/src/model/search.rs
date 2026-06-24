use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use file_index::{
    DirectoryErrorPolicy, FileSearchIndexStatus, FileSearchMatch, IndexProfile, MediaMetadataScope,
    SearchMode,
};
use tokio_util::sync::CancellationToken;

use crate::startup_index_tree::StartupIndexBuildRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchScope {
    CurrentDirectory,
    HomeDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchRequest {
    pub(crate) scope: SearchScope,
    pub(crate) mode: SearchMode,
    pub(crate) root: PathBuf,
    pub(crate) query: String,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchState {
    pub(crate) scope: SearchScope,
    pub(crate) mode: SearchMode,
    pub(crate) root: PathBuf,
    pub(crate) query: String,
    pub(crate) request_generation: u64,
    pub(crate) search_cancel: Option<CancellationToken>,
    pub(crate) matches: Vec<FileSearchMatch>,
    pub(crate) selected_match: Option<usize>,
    pub(crate) is_loading: bool,
    pub(crate) is_indexing: bool,
    pub(crate) skipped_count: usize,
    pub(crate) error: Option<String>,
    pub(crate) index_error: Option<String>,
}

impl SearchState {
    pub(crate) fn request(&self) -> SearchRequest {
        SearchRequest {
            scope: self.scope,
            mode: self.mode,
            root: self.root.clone(),
            query: self.query.clone(),
            generation: self.request_generation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchIndexPathRuleKind {
    Indexed,
    Excluded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchIndexPathRuleSelection {
    IndexedRoot(PathBuf),
    ExcludePattern(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexPathRuleListEntry {
    pub(crate) kind: SearchIndexPathRuleKind,
    pub(crate) selection: SearchIndexPathRuleSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchIndexPathRuleOrderEntry {
    IndexedRoot(PathBuf),
    ExcludePattern(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchIndexPathRuleEditMode {
    Adding,
    Modifying(SearchIndexPathRuleSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchIndexProfileSaveReason {
    General,
    StartupIndexSetup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchIndexDaemonStatus {
    Reachable,
    Unreachable(String),
}

#[derive(Debug, Clone)]
pub(crate) struct SearchIndexRuntime {
    pub(crate) base_dir: PathBuf,
    pub(crate) home_dir: PathBuf,
    pub(crate) profile_id: String,
    pub(crate) profile_roots: Vec<PathBuf>,
    pub(crate) profile_loading: bool,
    pub(crate) profile_error: Option<String>,
    pub(crate) maintenance_paused: bool,
    pub(crate) service_generation: u64,
    pub(crate) status_generation: u64,
    pub(crate) daemon_status: Option<SearchIndexDaemonStatus>,
    pub(crate) daemon_status_loading: bool,
    pub(crate) indexing_roots: HashSet<PathBuf>,
    pub(crate) pending_startup_index_builds: Vec<StartupIndexBuildRequest>,
    pub(crate) root_errors: HashMap<PathBuf, String>,
    pub(crate) statuses: HashMap<PathBuf, FileSearchIndexStatus>,
    pub(crate) status_loading_roots: HashMap<PathBuf, u64>,
    pub(crate) exclude_pattern_inputs: Vec<String>,
    path_rule_order: Vec<SearchIndexPathRuleOrderEntry>,
    pub(crate) selected_path_rule: Option<SearchIndexPathRuleSelection>,
    pub(crate) path_rule_editor: Option<SearchIndexPathRuleEditMode>,
    pub(crate) path_rule_input: String,
    pub(crate) path_rule_kind: SearchIndexPathRuleKind,
    pub(crate) directory_error_policy: DirectoryErrorPolicy,
    pub(crate) content_index_enabled: bool,
    pub(crate) media_metadata_scope: MediaMetadataScope,
}

impl SearchIndexRuntime {
    pub(crate) fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            home_dir: PathBuf::new(),
            profile_id: "default".to_owned(),
            profile_roots: Vec::new(),
            profile_loading: false,
            profile_error: None,
            maintenance_paused: false,
            service_generation: 0,
            status_generation: 0,
            daemon_status: None,
            daemon_status_loading: false,
            indexing_roots: HashSet::new(),
            pending_startup_index_builds: Vec::new(),
            root_errors: HashMap::new(),
            statuses: HashMap::new(),
            status_loading_roots: HashMap::new(),
            exclude_pattern_inputs: Vec::new(),
            path_rule_order: Vec::new(),
            selected_path_rule: None,
            path_rule_editor: None,
            path_rule_input: "~".to_owned(),
            path_rule_kind: SearchIndexPathRuleKind::Indexed,
            directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
            content_index_enabled: false,
            media_metadata_scope: MediaMetadataScope::Off,
        }
    }

    pub(crate) fn apply_profile(&mut self, profile: &IndexProfile) {
        self.profile_id = profile.id.clone();
        self.profile_roots = profile.roots.clone();
        self.directory_error_policy = profile.directory_error_policy;
        self.content_index_enabled = profile.content.enabled;
        self.media_metadata_scope = profile.media.scope;
        self.profile_loading = false;
        self.profile_error = None;
        self.service_generation = self.service_generation.wrapping_add(1);
        self.sync_path_rule_order_with_current_rules();
    }

    pub(crate) fn has_active_profile_roots(&self) -> bool {
        !self.profile_roots.is_empty()
    }

    pub(crate) fn path_rule_entries(&self) -> Vec<SearchIndexPathRuleListEntry> {
        let mut entries = Vec::new();
        let mut included_roots = HashSet::new();
        let mut included_patterns = HashSet::new();

        for order_entry in &self.path_rule_order {
            match order_entry {
                SearchIndexPathRuleOrderEntry::IndexedRoot(root)
                    if self.profile_roots.contains(root) && included_roots.insert(root.clone()) =>
                {
                    entries.push(SearchIndexPathRuleListEntry {
                        kind: SearchIndexPathRuleKind::Indexed,
                        selection: SearchIndexPathRuleSelection::IndexedRoot(root.clone()),
                    });
                }
                SearchIndexPathRuleOrderEntry::ExcludePattern(pattern) => {
                    let Some(index) = self
                        .exclude_pattern_inputs
                        .iter()
                        .position(|candidate| candidate == pattern)
                    else {
                        continue;
                    };
                    if included_patterns.insert(pattern.clone()) {
                        entries.push(SearchIndexPathRuleListEntry {
                            kind: SearchIndexPathRuleKind::Excluded,
                            selection: SearchIndexPathRuleSelection::ExcludePattern(index),
                        });
                    }
                }
                SearchIndexPathRuleOrderEntry::IndexedRoot(_) => {}
            }
        }

        for root in &self.profile_roots {
            if included_roots.insert(root.clone()) {
                entries.push(SearchIndexPathRuleListEntry {
                    kind: SearchIndexPathRuleKind::Indexed,
                    selection: SearchIndexPathRuleSelection::IndexedRoot(root.clone()),
                });
            }
        }
        for (index, pattern) in self.exclude_pattern_inputs.iter().enumerate() {
            if included_patterns.insert(pattern.clone()) {
                entries.push(SearchIndexPathRuleListEntry {
                    kind: SearchIndexPathRuleKind::Excluded,
                    selection: SearchIndexPathRuleSelection::ExcludePattern(index),
                });
            }
        }

        entries
    }

    pub(crate) fn reset_path_rule_order_from_current_rules(&mut self) {
        self.path_rule_order = self
            .profile_roots
            .iter()
            .cloned()
            .map(SearchIndexPathRuleOrderEntry::IndexedRoot)
            .chain(
                self.exclude_pattern_inputs
                    .iter()
                    .cloned()
                    .map(SearchIndexPathRuleOrderEntry::ExcludePattern),
            )
            .collect();
    }

    pub(crate) fn sync_path_rule_order_with_current_rules(&mut self) {
        self.path_rule_order = self
            .path_rule_entries()
            .into_iter()
            .filter_map(|entry| self.path_rule_order_entry(&entry.selection))
            .collect();
    }

    pub(crate) fn path_rule_order_entry(
        &self,
        selection: &SearchIndexPathRuleSelection,
    ) -> Option<SearchIndexPathRuleOrderEntry> {
        match selection {
            SearchIndexPathRuleSelection::IndexedRoot(root) => {
                Some(SearchIndexPathRuleOrderEntry::IndexedRoot(root.clone()))
            }
            SearchIndexPathRuleSelection::ExcludePattern(index) => self
                .exclude_pattern_inputs
                .get(*index)
                .cloned()
                .map(SearchIndexPathRuleOrderEntry::ExcludePattern),
        }
    }

    pub(crate) fn append_path_rule_to_order(&mut self, selection: &SearchIndexPathRuleSelection) {
        if let Some(entry) = self.path_rule_order_entry(selection) {
            if !self.path_rule_order.contains(&entry) {
                self.path_rule_order.push(entry);
            }
        }
        self.sync_path_rule_order_with_current_rules();
    }

    pub(crate) fn replace_path_rule_order_entry(
        &mut self,
        old_entry: Option<SearchIndexPathRuleOrderEntry>,
        selection: &SearchIndexPathRuleSelection,
    ) {
        let Some(new_entry) = self.path_rule_order_entry(selection) else {
            self.sync_path_rule_order_with_current_rules();
            return;
        };
        let replaced = old_entry
            .as_ref()
            .and_then(|entry| {
                self.path_rule_order
                    .iter()
                    .position(|candidate| candidate == entry)
            })
            .map(|position| {
                self.path_rule_order[position] = new_entry.clone();
            })
            .is_some();
        if !replaced && !self.path_rule_order.contains(&new_entry) {
            self.path_rule_order.push(new_entry);
        }
        self.sync_path_rule_order_with_current_rules();
    }

    pub(crate) fn remove_path_rule_order_entry(
        &mut self,
        entry: Option<SearchIndexPathRuleOrderEntry>,
    ) {
        if let Some(entry) = entry {
            self.path_rule_order.retain(|candidate| candidate != &entry);
        }
        self.sync_path_rule_order_with_current_rules();
    }
}
