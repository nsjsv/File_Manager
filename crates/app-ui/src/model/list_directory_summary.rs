use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListDirectorySizeDisplayMode {
    ItemCount,
    RecursiveTotalSize,
}

impl Default for ListDirectorySizeDisplayMode {
    fn default() -> Self {
        Self::ItemCount
    }
}

impl ListDirectorySizeDisplayMode {
    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::ItemCount => "item_count",
            Self::RecursiveTotalSize => "recursive_total_size",
        }
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "item_count" => Some(Self::ItemCount),
            "recursive_total_size" => Some(Self::RecursiveTotalSize),
            _ => None,
        }
    }

    pub(crate) fn uses_recursive_total_size(self) -> bool {
        matches!(self, Self::RecursiveTotalSize)
    }

    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::ItemCount => Self::RecursiveTotalSize,
            Self::RecursiveTotalSize => Self::ItemCount,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListDirectorySummary {
    pub(crate) direct_child_count: usize,
    pub(crate) recursive_total_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListDirectorySummaryLoadRequest {
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
    pub(crate) include_recursive_total_size: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ListDirectorySummaryCache {
    entries: HashMap<PathBuf, ListDirectorySummaryCacheEntry>,
}

#[derive(Debug, Clone, Default)]
struct ListDirectorySummaryCacheEntry {
    generation: u64,
    direct_child_count: Option<usize>,
    recursive_total_size_bytes: Option<u64>,
    direct_child_count_loading: bool,
    direct_child_count_failed: bool,
    recursive_total_size_loading: bool,
    recursive_total_size_failed: bool,
}

impl ListDirectorySummaryCache {
    pub(crate) fn summary_for_path(&self, path: &Path) -> Option<ListDirectorySummary> {
        let entry = self.entries.get(path)?;
        Some(ListDirectorySummary {
            direct_child_count: entry.direct_child_count?,
            recursive_total_size_bytes: entry.recursive_total_size_bytes,
        })
    }

    pub(crate) fn remember_direct_child_count(&mut self, path: PathBuf, direct_child_count: usize) {
        let entry = self.entries.entry(path).or_default();
        entry.direct_child_count = Some(direct_child_count);
        entry.direct_child_count_loading = false;
        entry.direct_child_count_failed = false;
    }

    pub(crate) fn start_request(
        &mut self,
        path: PathBuf,
        include_recursive_total_size: bool,
    ) -> Option<ListDirectorySummaryLoadRequest> {
        let entry = self.entries.entry(path.clone()).or_default();
        let direct_child_count_missing =
            entry.direct_child_count.is_none() && !entry.direct_child_count_failed;
        let recursive_total_size_missing = include_recursive_total_size
            && entry.recursive_total_size_bytes.is_none()
            && !entry.recursive_total_size_failed;
        let should_start_direct_request =
            direct_child_count_missing && !entry.direct_child_count_loading;
        let should_start_recursive_request =
            recursive_total_size_missing && !entry.recursive_total_size_loading;

        if !should_start_direct_request && !should_start_recursive_request {
            return None;
        }

        if should_start_direct_request {
            entry.direct_child_count_loading = true;
        }
        if should_start_recursive_request {
            entry.recursive_total_size_loading = true;
        }

        Some(ListDirectorySummaryLoadRequest {
            path,
            generation: entry.generation,
            include_recursive_total_size: should_start_recursive_request,
        })
    }

    pub(crate) fn store_summary(
        &mut self,
        request: &ListDirectorySummaryLoadRequest,
        summary: ListDirectorySummary,
    ) -> bool {
        let Some(entry) = self.entries.get_mut(&request.path) else {
            return false;
        };
        if entry.generation != request.generation {
            return false;
        }

        entry.direct_child_count = Some(summary.direct_child_count);
        entry.direct_child_count_loading = false;
        entry.direct_child_count_failed = false;
        if request.include_recursive_total_size {
            entry.recursive_total_size_bytes = summary.recursive_total_size_bytes;
            entry.recursive_total_size_loading = false;
            entry.recursive_total_size_failed = false;
        }
        true
    }

    pub(crate) fn store_failure(&mut self, request: &ListDirectorySummaryLoadRequest) -> bool {
        let Some(entry) = self.entries.get_mut(&request.path) else {
            return false;
        };
        if entry.generation != request.generation {
            return false;
        }

        entry.direct_child_count_loading = false;
        if entry.direct_child_count.is_none() {
            entry.direct_child_count_failed = true;
        }
        if request.include_recursive_total_size {
            entry.recursive_total_size_loading = false;
            entry.recursive_total_size_failed = true;
        }
        true
    }

    pub(crate) fn invalidate_path(&mut self, path: &Path) {
        let Some(entry) = self.entries.get_mut(path) else {
            return;
        };
        entry.generation = entry.generation.wrapping_add(1);
        entry.direct_child_count = None;
        entry.recursive_total_size_bytes = None;
        entry.direct_child_count_loading = false;
        entry.direct_child_count_failed = false;
        entry.recursive_total_size_loading = false;
        entry.recursive_total_size_failed = false;
    }

    pub(crate) fn invalidate_path_subtree(&mut self, path: &Path) {
        let affected_paths = self
            .entries
            .keys()
            .filter(|candidate| candidate.starts_with(path))
            .cloned()
            .collect::<Vec<_>>();
        for affected_path in affected_paths {
            self.invalidate_path(&affected_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_display_mode_is_item_count() {
        assert_eq!(
            ListDirectorySizeDisplayMode::default(),
            ListDirectorySizeDisplayMode::ItemCount
        );
        assert_eq!(
            ListDirectorySizeDisplayMode::RecursiveTotalSize.toggled(),
            ListDirectorySizeDisplayMode::ItemCount
        );
    }

    #[test]
    fn cache_deduplicates_direct_child_count_request_until_invalidation() {
        let path = PathBuf::from("/workspace/projects");
        let mut cache = ListDirectorySummaryCache::default();

        let first = cache
            .start_request(path.clone(), false)
            .expect("first request");
        assert!(!first.include_recursive_total_size);
        assert!(cache.start_request(path.clone(), false).is_none());

        assert!(cache.store_summary(
            &first,
            ListDirectorySummary {
                direct_child_count: 4,
                recursive_total_size_bytes: None,
            }
        ));
        assert_eq!(cache.summary_for_path(&path).unwrap().direct_child_count, 4);
        assert!(cache.start_request(path.clone(), false).is_none());

        cache.invalidate_path(&path);

        let second = cache
            .start_request(path.clone(), false)
            .expect("request after invalidation");
        assert_ne!(first.generation, second.generation);
    }

    #[test]
    fn recursive_request_can_start_while_direct_count_load_is_in_flight() {
        let path = PathBuf::from("/workspace/projects");
        let mut cache = ListDirectorySummaryCache::default();

        let direct_only = cache
            .start_request(path.clone(), false)
            .expect("direct count request");
        let recursive = cache
            .start_request(path.clone(), true)
            .expect("recursive request");

        assert!(!direct_only.include_recursive_total_size);
        assert!(recursive.include_recursive_total_size);
        assert!(cache.start_request(path.clone(), true).is_none());
    }

    #[test]
    fn stale_results_are_ignored_after_invalidation() {
        let path = PathBuf::from("/workspace/projects");
        let mut cache = ListDirectorySummaryCache::default();
        let request = cache
            .start_request(path.clone(), true)
            .expect("initial request");

        cache.invalidate_path(&path);

        assert!(!cache.store_summary(
            &request,
            ListDirectorySummary {
                direct_child_count: 9,
                recursive_total_size_bytes: Some(4096),
            }
        ));
        assert!(cache.summary_for_path(&path).is_none());
    }

    #[test]
    fn failed_request_is_not_retried_until_invalidation() {
        let path = PathBuf::from("/workspace/projects");
        let mut cache = ListDirectorySummaryCache::default();

        let request = cache
            .start_request(path.clone(), true)
            .expect("initial request");
        assert!(cache.store_failure(&request));
        assert!(cache.start_request(path.clone(), true).is_none());

        cache.invalidate_path(&path);

        assert!(cache.start_request(path, true).is_some());
    }

    #[test]
    fn invalidating_subtree_clears_descendants_only() {
        let root = PathBuf::from("/workspace");
        let project = root.join("project");
        let src = project.join("src");
        let nested = src.join("nested");
        let unrelated = root.join("notes");
        let mut cache = ListDirectorySummaryCache::default();

        for path in [
            project.clone(),
            src.clone(),
            nested.clone(),
            unrelated.clone(),
        ] {
            let request = cache
                .start_request(path.clone(), true)
                .expect("initial request");
            assert!(cache.store_summary(
                &request,
                ListDirectorySummary {
                    direct_child_count: 1,
                    recursive_total_size_bytes: Some(128),
                }
            ));
        }

        cache.invalidate_path_subtree(&src);

        assert!(cache.summary_for_path(&project).is_some());
        assert!(cache.summary_for_path(&src).is_none());
        assert!(cache.summary_for_path(&nested).is_none());
        assert!(cache.summary_for_path(&unrelated).is_some());
    }

    #[test]
    fn failed_recursive_request_keeps_known_direct_count() {
        let path = PathBuf::from("/workspace/projects");
        let mut cache = ListDirectorySummaryCache::default();
        cache.remember_direct_child_count(path.clone(), 4);

        let request = cache
            .start_request(path.clone(), true)
            .expect("recursive request");
        assert!(cache.store_failure(&request));

        let summary = cache
            .summary_for_path(&path)
            .expect("summary after failure");
        assert_eq!(summary.direct_child_count, 4);
        assert_eq!(summary.recursive_total_size_bytes, None);
        assert!(cache.start_request(path, true).is_none());
    }
}
