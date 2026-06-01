use std::cmp::Ordering;
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use crate::{DirectoryEntry, FileKind, ScanOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Name,
    Size,
    Kind,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

pub fn apply_entry_options(entries: &mut Vec<DirectoryEntry>, options: &ScanOptions) {
    filter_hidden(entries, options.include_hidden);
    sort_entries(entries, options);
}

pub fn filter_hidden(entries: &mut Vec<DirectoryEntry>, include_hidden: bool) {
    if !include_hidden {
        entries.retain(|entry| !entry.is_hidden);
    }
}

pub fn sort_entries(entries: &mut [DirectoryEntry], options: &ScanOptions) {
    entries.sort_unstable_by(|left, right| compare_entries(left, right, options));
}

pub fn compare_entries(
    left: &DirectoryEntry,
    right: &DirectoryEntry,
    options: &ScanOptions,
) -> Ordering {
    if options.directories_first {
        match (
            left.kind == FileKind::Directory,
            right.kind == FileKind::Directory,
        ) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }

    let ordering = match options.sort_field {
        SortField::Name => compare_names(left.name(), right.name()),
        SortField::Size => left
            .metadata
            .len
            .cmp(&right.metadata.len)
            .then_with(|| compare_names(left.name(), right.name())),
        SortField::Kind => kind_rank(left.kind)
            .cmp(&kind_rank(right.kind))
            .then_with(|| compare_names(left.name(), right.name())),
        SortField::Modified => left
            .metadata
            .modified
            .cmp(&right.metadata.modified)
            .then_with(|| compare_names(left.name(), right.name())),
    };

    match options.sort_direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

fn kind_rank(kind: FileKind) -> u8 {
    match kind {
        FileKind::Directory => 0,
        FileKind::File => 1,
        FileKind::Symlink => 2,
        FileKind::Other => 3,
    }
}

#[cfg(unix)]
fn compare_names(left: &OsStr, right: &OsStr) -> Ordering {
    left.as_bytes().cmp(right.as_bytes())
}

#[cfg(not(unix))]
fn compare_names(left: &OsStr, right: &OsStr) -> Ordering {
    left.to_string_lossy().cmp(&right.to_string_lossy())
}
