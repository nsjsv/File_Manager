use std::path::PathBuf;

use file_index::FileSearchIndexFailure;
use iced::{clipboard, Task};

use crate::app::FileBrowser;
use crate::model::{Message, SearchIndexDaemonStatus, SearchIndexErrorCopyTarget};

use super::search_index_display_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexIssueReport {
    pub(crate) daemon_error: Option<String>,
    pub(crate) profile_error: Option<String>,
    pub(crate) roots: Vec<SearchIndexIssueRoot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexIssueRoot {
    pub(crate) root: PathBuf,
    pub(crate) root_error: Option<String>,
    pub(crate) failures: Vec<FileSearchIndexFailure>,
}

impl SearchIndexIssueReport {
    pub(crate) fn issue_count(&self) -> usize {
        let global =
            usize::from(self.daemon_error.is_some()) + usize::from(self.profile_error.is_some());
        global
            + self
                .roots
                .iter()
                .map(|root| usize::from(root.root_error.is_some()) + root.failures.len())
                .sum::<usize>()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.issue_count() == 0
    }
}

impl FileBrowser {
    pub(crate) fn search_index_issue_report(&self) -> SearchIndexIssueReport {
        let daemon_error = match self.search_index.daemon_status.as_ref() {
            Some(SearchIndexDaemonStatus::Unreachable(error)) => Some(error.clone()),
            _ => None,
        };

        let mut roots = Vec::new();
        for root in self.search_index_setting_roots() {
            let root_error = self.search_index.root_errors.get(&root).cloned();
            let failures = self
                .search_index
                .statuses
                .get(&root)
                .map(|status| status.failures.clone())
                .unwrap_or_default();

            if root_error.is_none() && failures.is_empty() {
                continue;
            }

            roots.push(SearchIndexIssueRoot {
                root,
                root_error,
                failures,
            });
        }

        SearchIndexIssueReport {
            daemon_error,
            profile_error: self.search_index.profile_error.clone(),
            roots,
        }
    }

    pub(crate) fn copy_search_index_error(
        &mut self,
        target: SearchIndexErrorCopyTarget,
    ) -> Task<Message> {
        let report = self.search_index_issue_report();
        let Some(contents) = self.search_index_error_text(&report, &target) else {
            return Task::none();
        };

        clipboard::write(contents)
    }

    fn search_index_error_text(
        &self,
        report: &SearchIndexIssueReport,
        target: &SearchIndexErrorCopyTarget,
    ) -> Option<String> {
        match target {
            SearchIndexErrorCopyTarget::All => format_full_search_index_issue_report(self, report),
            SearchIndexErrorCopyTarget::DaemonStatus => report
                .daemon_error
                .as_ref()
                .map(|error| format!("Service error: {error}")),
            SearchIndexErrorCopyTarget::ProfileError => report
                .profile_error
                .as_ref()
                .map(|error| format!("Profile error: {error}")),
            SearchIndexErrorCopyTarget::RootError(root) => report
                .roots
                .iter()
                .find(|group| &group.root == root)
                .and_then(|group| {
                    group.root_error.as_ref().map(|error| {
                        format!(
                            "{}\nRoot error: {error}",
                            search_index_display_path(root, &self.search_index_home_directory())
                        )
                    })
                }),
            SearchIndexErrorCopyTarget::Failure { root, index } => report
                .roots
                .iter()
                .find(|group| &group.root == root)
                .and_then(|group| group.failures.get(*index))
                .map(|failure| format_search_index_failure(self, root, failure, Some("Failure"))),
        }
    }
}

fn format_full_search_index_issue_report(
    browser: &FileBrowser,
    report: &SearchIndexIssueReport,
) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(error) = &report.daemon_error {
        lines.push(format!("Service error: {error}"));
    }
    if let Some(error) = &report.profile_error {
        lines.push(format!("Profile error: {error}"));
    }

    for group in &report.roots {
        let title = search_index_display_path(&group.root, &browser.search_index_home_directory());
        lines.push(title);
        if let Some(error) = &group.root_error {
            lines.push(format!("  Root error: {error}"));
        }
        for failure in &group.failures {
            let failure_text = format_search_index_failure(browser, &group.root, failure, None);
            for line in failure_text.lines() {
                lines.push(format!("  {line}"));
            }
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn format_search_index_failure(
    browser: &FileBrowser,
    root: &PathBuf,
    failure: &FileSearchIndexFailure,
    label: Option<&str>,
) -> String {
    let root_label = search_index_display_path(root, &browser.search_index_home_directory());
    let path_label =
        search_index_display_path(&failure.path, &browser.search_index_home_directory());
    match label {
        Some(label) => format!("{root_label}\n{label}: {path_label}\n{}", failure.message),
        None => format!("{path_label}\n{}", failure.message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use file_index::{FileSearchIndexStatus, MediaMetadataScope};

    #[test]
    fn issue_report_groups_daemon_profile_root_and_failure_entries() {
        let mut browser = browser_with_search_index_state();
        let root = PathBuf::from("/home/user/Project");
        browser.search_index.daemon_status = Some(SearchIndexDaemonStatus::Unreachable(
            "socket unavailable".to_owned(),
        ));
        browser.search_index.profile_error = Some("profile missing".to_owned());
        browser
            .search_index
            .root_errors
            .insert(root.clone(), "root missing".to_owned());
        browser
            .search_index
            .statuses
            .insert(root.clone(), status_with_failure(root.clone()));

        let report = browser.search_index_issue_report();

        assert_eq!(report.daemon_error.as_deref(), Some("socket unavailable"));
        assert_eq!(report.profile_error.as_deref(), Some("profile missing"));
        assert_eq!(report.issue_count(), 4);
        assert_eq!(report.roots.len(), 1);
        assert_eq!(report.roots[0].root, root);
        assert_eq!(report.roots[0].root_error.as_deref(), Some("root missing"));
        assert_eq!(report.roots[0].failures.len(), 1);
    }

    #[test]
    fn copy_all_text_uses_same_grouping_as_errors_page() {
        let mut browser = browser_with_search_index_state();
        let root = PathBuf::from("/home/user/Project");
        browser.search_index.profile_error = Some("profile missing".to_owned());
        browser
            .search_index
            .root_errors
            .insert(root.clone(), "root missing".to_owned());
        browser
            .search_index
            .statuses
            .insert(root.clone(), status_with_failure(root.clone()));
        let report = browser.search_index_issue_report();

        let text = browser
            .search_index_error_text(&report, &SearchIndexErrorCopyTarget::All)
            .expect("copy text");

        assert!(text.contains("Profile error: profile missing"));
        assert!(text.contains("~/Project"));
        assert!(text.contains("Root error: root missing"));
        assert!(text.contains("~/Project/blocked"));
        assert!(text.contains("permission denied"));
    }

    fn browser_with_search_index_state() -> FileBrowser {
        let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
        browser.search_index.home_dir = PathBuf::from("/home/user");
        browser.search_index.profile_roots = vec![PathBuf::from("/home/user/Project")];
        browser
    }

    fn status_with_failure(root: PathBuf) -> FileSearchIndexStatus {
        FileSearchIndexStatus {
            root,
            index_dir: PathBuf::from("/tmp/index"),
            exists: true,
            stale: false,
            reason: None,
            include_hidden: false,
            content_index_enabled: false,
            content_max_file_bytes: 16 * 1024 * 1024,
            media_metadata_scope: MediaMetadataScope::Off,
            record_count: 0,
            index_size_bytes: 0,
            built_at_ms: None,
            updated_at_ms: None,
            failed_count: 1,
            exclude_rules_hash: None,
            extractor_version: None,
            failures: vec![FileSearchIndexFailure {
                path: PathBuf::from("/home/user/Project/blocked"),
                message: "permission denied".to_owned(),
                first_failed_at_ms: 1,
                last_failed_at_ms: 2,
                retry_count: 0,
            }],
        }
    }
}
