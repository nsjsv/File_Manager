use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use file_index::FileSearchIndexStatus;

use crate::config::normalize_search_index_exclude_patterns;
use crate::model::{SearchIndexPathRuleKind, SearchIndexPathRuleSelection, SearchIndexRuntime};

const EXCLUDE_REQUIRES_INDEXED_PARENT_ERROR: &str =
    "Add an indexed parent path before excluding this path.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathRuleChange {
    pub(super) selection: SearchIndexPathRuleSelection,
    pub(super) roots_changed: bool,
    pub(super) excludes_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PathRuleRemoval {
    pub(super) roots_changed: bool,
    pub(super) excludes_changed: bool,
}

pub(super) fn search_index_path_rule_input_is_valid(
    input: &str,
    kind: SearchIndexPathRuleKind,
    roots: &[PathBuf],
    home: &Path,
) -> bool {
    match kind {
        SearchIndexPathRuleKind::Indexed => search_index_path_from_input(input, home).is_ok(),
        SearchIndexPathRuleKind::Excluded => {
            search_index_exclude_pattern_from_input(input, roots, home).is_ok()
        }
    }
}

pub(super) fn add_path_rule_to_search_index(
    roots: &mut Vec<PathBuf>,
    exclude_patterns: &mut Vec<String>,
    input: &str,
    kind: SearchIndexPathRuleKind,
    home: &Path,
) -> Result<PathRuleChange, String> {
    match kind {
        SearchIndexPathRuleKind::Indexed => {
            add_index_root_path_rule(roots, search_index_path_from_input(input, home)?)
        }
        SearchIndexPathRuleKind::Excluded => add_exclude_pattern_rule(
            exclude_patterns,
            search_index_exclude_pattern_from_input(input, roots, home)?,
        ),
    }
}

pub(super) fn update_path_rule_in_search_index(
    roots: &mut Vec<PathBuf>,
    exclude_patterns: &mut Vec<String>,
    statuses: &mut HashMap<PathBuf, FileSearchIndexStatus>,
    errors: &mut HashMap<PathBuf, String>,
    selection: &SearchIndexPathRuleSelection,
    input: &str,
    kind: SearchIndexPathRuleKind,
    home: &Path,
) -> Result<PathRuleChange, String> {
    let old_roots = roots.clone();
    let old_excludes = exclude_patterns.clone();
    let mut next_roots = roots.clone();
    let mut next_excludes = exclude_patterns.clone();
    let mut next_statuses = statuses.clone();
    let mut next_errors = errors.clone();
    let removal = remove_path_rule_from_search_index(
        &mut next_roots,
        &mut next_excludes,
        &mut next_statuses,
        &mut next_errors,
        selection,
    );
    let mut change = match kind {
        SearchIndexPathRuleKind::Indexed => {
            add_index_root_path_rule(&mut next_roots, search_index_path_from_input(input, home)?)
        }
        SearchIndexPathRuleKind::Excluded => add_exclude_pattern_rule(
            &mut next_excludes,
            search_index_exclude_pattern_from_input(input, &next_roots, home)?,
        ),
    }?;
    change.roots_changed |= removal.roots_changed || next_roots != old_roots;
    change.excludes_changed |= removal.excludes_changed || next_excludes != old_excludes;
    *roots = next_roots;
    *exclude_patterns = next_excludes;
    *statuses = next_statuses;
    *errors = next_errors;
    Ok(change)
}

fn add_index_root_path_rule(
    roots: &mut Vec<PathBuf>,
    root: PathBuf,
) -> Result<PathRuleChange, String> {
    let roots_changed = if roots.contains(&root) {
        false
    } else {
        roots.push(root.clone());
        true
    };

    Ok(PathRuleChange {
        selection: SearchIndexPathRuleSelection::IndexedRoot(root),
        roots_changed,
        excludes_changed: false,
    })
}

fn add_exclude_pattern_rule(
    exclude_patterns: &mut Vec<String>,
    pattern: String,
) -> Result<PathRuleChange, String> {
    let mut normalized = exclude_patterns.clone();
    normalized.push(pattern.clone());
    normalized = normalize_search_index_exclude_patterns(normalized);
    let excludes_changed = normalized != *exclude_patterns;
    *exclude_patterns = normalized;
    let index = exclude_patterns
        .iter()
        .position(|candidate| candidate == &pattern)
        .unwrap_or(exclude_patterns.len().saturating_sub(1));

    Ok(PathRuleChange {
        selection: SearchIndexPathRuleSelection::ExcludePattern(index),
        roots_changed: false,
        excludes_changed,
    })
}

fn search_index_exclude_pattern_from_input(
    input: &str,
    roots: &[PathBuf],
    home: &Path,
) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter an ignore pattern or path.".to_owned());
    }
    if let Some(path) = search_index_exclude_path_from_input(trimmed, home)? {
        return search_index_exclude_pattern_for_path(roots, &path)
            .ok_or_else(|| EXCLUDE_REQUIRES_INDEXED_PARENT_ERROR.to_owned());
    }
    normalize_search_index_exclude_patterns(vec![trimmed.to_owned()])
        .into_iter()
        .next()
        .ok_or_else(|| "Enter an ignore pattern or path.".to_owned())
}

fn search_index_exclude_path_from_input(
    input: &str,
    home: &Path,
) -> Result<Option<PathBuf>, String> {
    if input == "~" || input.starts_with("~/") {
        return search_index_path_from_input(input, home).map(Some);
    }
    let path = PathBuf::from(input);
    if !path.is_absolute() {
        return Ok(None);
    }
    normalize_search_index_path(&path).map(Some)
}

pub(super) fn remove_path_rule_from_search_index(
    roots: &mut Vec<PathBuf>,
    exclude_patterns: &mut Vec<String>,
    statuses: &mut HashMap<PathBuf, FileSearchIndexStatus>,
    errors: &mut HashMap<PathBuf, String>,
    selection: &SearchIndexPathRuleSelection,
) -> PathRuleRemoval {
    match selection {
        SearchIndexPathRuleSelection::IndexedRoot(root) => {
            let old_len = roots.len();
            roots.retain(|profile_root| profile_root != root);
            statuses.remove(root);
            errors.remove(root);
            PathRuleRemoval {
                roots_changed: roots.len() != old_len,
                excludes_changed: false,
            }
        }
        SearchIndexPathRuleSelection::ExcludePattern(index) => {
            if *index < exclude_patterns.len() {
                exclude_patterns.remove(*index);
                PathRuleRemoval {
                    roots_changed: false,
                    excludes_changed: true,
                }
            } else {
                PathRuleRemoval {
                    roots_changed: false,
                    excludes_changed: false,
                }
            }
        }
    }
}

pub(super) fn search_index_path_rule_input(
    runtime: &SearchIndexRuntime,
    selection: &SearchIndexPathRuleSelection,
    home: &Path,
) -> Option<String> {
    match selection {
        SearchIndexPathRuleSelection::IndexedRoot(root) => {
            Some(search_index_display_path(root, home))
        }
        SearchIndexPathRuleSelection::ExcludePattern(index) => runtime
            .exclude_pattern_inputs
            .get(*index)
            .and_then(|pattern| {
                search_index_exclude_pattern_display_path(&runtime.profile_roots, pattern, home)
            }),
    }
}

pub(crate) fn search_index_display_path(path: &Path, home: &Path) -> String {
    if path == home {
        return "~".to_owned();
    }
    path.strip_prefix(home)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| format!("~/{}", relative.to_string_lossy()))
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub(crate) fn search_index_exclude_pattern_display_path(
    roots: &[PathBuf],
    pattern: &str,
    home: &Path,
) -> Option<String> {
    let trimmed = pattern.trim();
    if !trimmed.starts_with('/') {
        return Some(trimmed.to_owned());
    }
    let relative = trimmed
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix('/'))?;
    if relative.is_empty() || relative.contains('*') || relative.contains('?') {
        return Some(trimmed.to_owned());
    }
    roots
        .first()
        .map(|root| search_index_display_path(&root.join(relative), home))
        .or_else(|| Some(trimmed.to_owned()))
}

fn search_index_path_from_input(input: &str, home: &Path) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter a path under your home directory.".to_owned());
    }

    let path = if trimmed == "~" {
        home.to_path_buf()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        home.join(rest)
    } else {
        let raw_path = PathBuf::from(trimmed);
        if raw_path.is_absolute() {
            raw_path
        } else {
            home.join(raw_path)
        }
    };
    let normalized = normalize_search_index_path(&path)?;
    if !root_is_inside_home(&normalized, home) {
        return Err("Only paths under your home directory can be indexed.".to_owned());
    }
    Ok(normalized)
}

fn normalize_search_index_path(path: &Path) -> Result<PathBuf, String> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                return Err(
                    "Use a path under your home directory without '..' segments.".to_owned(),
                )
            }
        }
    }
    Ok(normalized)
}

pub(super) fn root_is_inside_home(root: &Path, home: &Path) -> bool {
    root == home || root.starts_with(home)
}

pub(super) fn search_index_exclude_pattern_for_path(
    roots: &[PathBuf],
    path: &Path,
) -> Option<String> {
    let root = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())?;
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    Some(format!("/{}/", relative.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclude_pattern_for_path_is_relative_to_deepest_index_root() {
        let pattern = search_index_exclude_pattern_for_path(
            &[
                PathBuf::from("/workspace"),
                PathBuf::from("/workspace/project"),
            ],
            Path::new("/workspace/project/target"),
        );

        assert_eq!(pattern.as_deref(), Some("/target/"));
    }

    #[test]
    fn home_relative_input_resolves_under_home() {
        let path = search_index_path_from_input("~/Documents", Path::new("/home/user")).unwrap();

        assert_eq!(path, PathBuf::from("/home/user/Documents"));
    }

    #[test]
    fn absolute_input_outside_home_is_rejected() {
        let error = search_index_path_from_input("/tmp/cache", Path::new("/home/user"))
            .expect_err("outside home");

        assert_eq!(
            error,
            "Only paths under your home directory can be indexed."
        );
    }

    #[test]
    fn parent_segments_are_rejected_before_home_check() {
        let error = search_index_path_from_input("~/../tmp", Path::new("/home/user"))
            .expect_err("parent segment");

        assert_eq!(
            error,
            "Use a path under your home directory without '..' segments."
        );
    }

    #[test]
    fn excluded_path_requires_indexed_parent() {
        assert!(!search_index_path_rule_input_is_valid(
            "~/target",
            SearchIndexPathRuleKind::Excluded,
            &[],
            Path::new("/home/user"),
        ));
    }

    #[test]
    fn indexed_root_cannot_turn_into_exclude_without_remaining_parent() {
        let mut roots = vec![PathBuf::from("/home/user")];
        let mut excludes = Vec::new();
        let mut statuses = HashMap::new();
        let mut errors = HashMap::new();

        let error = update_path_rule_in_search_index(
            &mut roots,
            &mut excludes,
            &mut statuses,
            &mut errors,
            &SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user")),
            "~",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
        )
        .expect_err("missing indexed parent");

        assert_eq!(error, EXCLUDE_REQUIRES_INDEXED_PARENT_ERROR);
        assert_eq!(roots, vec![PathBuf::from("/home/user")]);
        assert!(excludes.is_empty());
    }

    #[test]
    fn indexed_and_excluded_rules_share_single_kind_per_row() {
        let mut roots = Vec::new();
        let mut excludes = Vec::new();
        let home = Path::new("/home/user");

        let root_change = add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "~",
            SearchIndexPathRuleKind::Indexed,
            home,
        )
        .unwrap();
        let exclude_change = add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "~/target",
            SearchIndexPathRuleKind::Excluded,
            home,
        )
        .unwrap();

        assert_eq!(
            root_change.selection,
            SearchIndexPathRuleSelection::IndexedRoot(PathBuf::from("/home/user"))
        );
        assert_eq!(
            exclude_change.selection,
            SearchIndexPathRuleSelection::ExcludePattern(0)
        );
        assert_eq!(excludes, vec!["/target/".to_owned()]);
    }

    #[test]
    fn global_exclude_pattern_input_is_kept_as_pattern() {
        let mut roots = vec![PathBuf::from("/home/user")];
        let mut excludes = Vec::new();

        let change = add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "node_modules/",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
        )
        .unwrap();

        assert_eq!(excludes, vec!["node_modules/".to_owned()]);
        assert_eq!(
            change.selection,
            SearchIndexPathRuleSelection::ExcludePattern(0)
        );
    }

    #[test]
    fn home_path_exclude_input_is_converted_to_root_relative_pattern() {
        let mut roots = vec![PathBuf::from("/home/user/Project")];
        let mut excludes = Vec::new();

        add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "~/Project/target",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
        )
        .unwrap();

        assert_eq!(excludes, vec!["/target/".to_owned()]);
    }

    #[test]
    fn absolute_home_exclude_input_is_converted_to_root_relative_pattern() {
        let mut roots = vec![PathBuf::from("/home/user")];
        let mut excludes = Vec::new();

        add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "/home/user/cache/",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
        )
        .unwrap();

        assert_eq!(excludes, vec!["/cache/".to_owned()]);
    }

    #[test]
    fn outside_absolute_exclude_input_requires_indexed_parent() {
        let mut roots = vec![PathBuf::from("/home/user")];
        let mut excludes = Vec::new();

        let error = add_path_rule_to_search_index(
            &mut roots,
            &mut excludes,
            "/var/cache/",
            SearchIndexPathRuleKind::Excluded,
            Path::new("/home/user"),
        )
        .expect_err("outside indexed parent");

        assert_eq!(error, EXCLUDE_REQUIRES_INDEXED_PARENT_ERROR);
        assert!(excludes.is_empty());
    }
}
