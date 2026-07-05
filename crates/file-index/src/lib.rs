pub mod daemon;
pub mod ipc;
mod layout;
pub mod profile;
pub mod search;
pub mod service;

use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("could not read directory {path:?}: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not use search index {path:?}: {message}")]
    Store { path: PathBuf, message: String },
    #[error("operation cancelled")]
    Cancelled,
}

impl IndexError {
    pub(crate) fn store(path: impl Into<PathBuf>, error: impl ToString) -> Self {
        let path = path.into();
        let message = error.to_string();
        Self::Store { path, message }
    }
}

pub use ipc::{default_socket_path, IndexClient, IndexClientError, INDEX_PROTOCOL_VERSION};
pub use layout::search_index_dir_for_root;
pub use profile::{
    IndexProfile, IndexTaskPhase, IndexTaskStatus, MediaMetadataPolicy, MediaMetadataScope,
    ProfileStore, SearchMode,
};
pub use search::{
    build_file_search_index, build_file_search_index_for_paths,
    build_file_search_index_for_paths_with_progress, clear_file_search_index_failures,
    default_search_index_exclude_patterns, file_search_index_exists, file_search_index_snapshot,
    file_search_index_status, remove_file_search_index, search_file_contents,
    search_file_contents_with_cancel, search_file_index, search_file_index_with_cancel,
    search_file_tree, search_file_tree_with_cancel, DirectoryErrorPolicy, FileSearchIndexFailure,
    FileSearchIndexOptions, FileSearchIndexOutcome, FileSearchIndexProgress, FileSearchIndexStatus,
    FileSearchMatch, FileSearchOptions, FileSearchOutcome, MediaExifField, MediaSearchKind,
    MediaSearchMetadata, SearchIndexFileRecord, SearchResultSource,
};
pub use service::{
    BuildSelectedPathsRequest, IndexService, IndexServiceCommand, IndexServiceCore,
    IndexServiceEvent, SearchQuery,
};
