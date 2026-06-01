use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PasteTargetMode {
    Copy,
    Move,
}

pub(super) fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(super) fn completed_path_text(path: &Path) -> String {
    let mut text = path_text(path);
    if !text.ends_with(std::path::MAIN_SEPARATOR) {
        text.push(std::path::MAIN_SEPARATOR);
    }
    text
}

pub(super) fn transfer_targets(
    directory: &Path,
    sources: &[PathBuf],
    mode: PasteTargetMode,
) -> Vec<(PathBuf, PathBuf)> {
    let mut reserved_targets = HashSet::new();
    sources
        .iter()
        .map(|source| {
            let candidate = child_path(directory, source);
            if mode == PasteTargetMode::Move && candidate == *source {
                reserved_targets.insert(candidate.clone());
                return (source.clone(), source.clone());
            }
            let target = if reserved_targets.contains(&candidate) {
                unique_alternate_path(&candidate, &mut reserved_targets)
            } else {
                reserved_targets.insert(candidate.clone());
                candidate
            };
            (source.clone(), target)
        })
        .collect()
}

fn child_path(directory: &Path, source: &Path) -> PathBuf {
    directory.join(transfer_source_name(source))
}

fn transfer_source_name(source: &Path) -> OsString {
    source
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("item"))
}

pub(super) fn unique_alternate_path(
    target: &Path,
    reserved_targets: &mut HashSet<PathBuf>,
) -> PathBuf {
    if !reserved_targets.contains(target) {
        let target = target.to_path_buf();
        reserved_targets.insert(target.clone());
        return target;
    }

    let parent = target.parent().unwrap_or_else(|| Path::new(""));
    let name = target
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("item"));

    for index in 1..1000 {
        let mut next = name.clone();
        next.push(format!(".copy{index}"));
        let candidate = parent.join(next);
        if !reserved_targets.contains(&candidate) {
            reserved_targets.insert(candidate.clone());
            return candidate;
        }
    }

    target.to_path_buf()
}
