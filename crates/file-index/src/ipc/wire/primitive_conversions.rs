use file_core::FileKind;

use crate::profile::SearchMode;
use crate::search::{
    DirectoryErrorPolicy, FileSearchIndexMode, FileSearchIndexProgress, MediaSearchKind,
    SearchResultSource,
};

use super::{
    WireDirectoryErrorPolicy, WireFileKind, WireFileSearchIndexMode, WireFileSearchIndexProgress,
    WireMediaSearchKind, WireSearchMode, WireSearchResultSource,
};

impl From<SearchMode> for WireSearchMode {
    fn from(mode: SearchMode) -> Self {
        match mode {
            SearchMode::Files => Self::Files,
            SearchMode::Contents => Self::Contents,
            SearchMode::Media => Self::Media,
            SearchMode::All => Self::All,
        }
    }
}

impl From<WireSearchMode> for SearchMode {
    fn from(mode: WireSearchMode) -> Self {
        match mode {
            WireSearchMode::Files => Self::Files,
            WireSearchMode::Contents => Self::Contents,
            WireSearchMode::Media => Self::Media,
            WireSearchMode::All => Self::All,
        }
    }
}

impl From<DirectoryErrorPolicy> for WireDirectoryErrorPolicy {
    fn from(policy: DirectoryErrorPolicy) -> Self {
        match policy {
            DirectoryErrorPolicy::Abort => Self::Abort,
            DirectoryErrorPolicy::SkipUnreadable => Self::SkipUnreadable,
        }
    }
}

impl From<WireDirectoryErrorPolicy> for DirectoryErrorPolicy {
    fn from(policy: WireDirectoryErrorPolicy) -> Self {
        match policy {
            WireDirectoryErrorPolicy::Abort => Self::Abort,
            WireDirectoryErrorPolicy::SkipUnreadable => Self::SkipUnreadable,
        }
    }
}

impl From<FileSearchIndexMode> for WireFileSearchIndexMode {
    fn from(mode: FileSearchIndexMode) -> Self {
        match mode {
            FileSearchIndexMode::FullRebuild => Self::FullRebuild,
            FileSearchIndexMode::Incremental => Self::Incremental,
        }
    }
}

impl From<WireFileSearchIndexMode> for FileSearchIndexMode {
    fn from(mode: WireFileSearchIndexMode) -> Self {
        match mode {
            WireFileSearchIndexMode::FullRebuild => Self::FullRebuild,
            WireFileSearchIndexMode::Incremental => Self::Incremental,
        }
    }
}

impl From<FileSearchIndexProgress> for WireFileSearchIndexProgress {
    fn from(progress: FileSearchIndexProgress) -> Self {
        match progress {
            FileSearchIndexProgress::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            } => Self::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            },
        }
    }
}

impl From<WireFileSearchIndexProgress> for FileSearchIndexProgress {
    fn from(progress: WireFileSearchIndexProgress) -> Self {
        match progress {
            WireFileSearchIndexProgress::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            } => Self::IndexedPaths {
                completed_paths,
                total_paths,
                indexed_count,
            },
        }
    }
}

impl From<FileKind> for WireFileKind {
    fn from(kind: FileKind) -> Self {
        match kind {
            FileKind::Directory => Self::Directory,
            FileKind::File => Self::File,
            FileKind::Symlink => Self::Symlink,
            FileKind::Other => Self::Other,
        }
    }
}

impl From<WireFileKind> for FileKind {
    fn from(kind: WireFileKind) -> Self {
        match kind {
            WireFileKind::Directory => Self::Directory,
            WireFileKind::File => Self::File,
            WireFileKind::Symlink => Self::Symlink,
            WireFileKind::Other => Self::Other,
        }
    }
}

impl From<SearchResultSource> for WireSearchResultSource {
    fn from(source: SearchResultSource) -> Self {
        match source {
            SearchResultSource::Files => Self::Files,
            SearchResultSource::Contents => Self::Contents,
            SearchResultSource::Media => Self::Media,
        }
    }
}

impl From<WireSearchResultSource> for SearchResultSource {
    fn from(source: WireSearchResultSource) -> Self {
        match source {
            WireSearchResultSource::Files => Self::Files,
            WireSearchResultSource::Contents => Self::Contents,
            WireSearchResultSource::Media => Self::Media,
        }
    }
}

impl From<MediaSearchKind> for WireMediaSearchKind {
    fn from(kind: MediaSearchKind) -> Self {
        match kind {
            MediaSearchKind::Image => Self::Image,
            MediaSearchKind::Audio => Self::Audio,
            MediaSearchKind::Video => Self::Video,
        }
    }
}

impl From<WireMediaSearchKind> for MediaSearchKind {
    fn from(kind: WireMediaSearchKind) -> Self {
        match kind {
            WireMediaSearchKind::Image => Self::Image,
            WireMediaSearchKind::Audio => Self::Audio,
            WireMediaSearchKind::Video => Self::Video,
        }
    }
}
