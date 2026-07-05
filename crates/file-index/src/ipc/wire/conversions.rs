use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use file_core::ScanWarning;

use crate::profile::{IndexProfile, MediaMetadataPolicy};
use crate::search::path_encoding::{path_from_bytes, path_to_bytes};
use crate::search::{
    FileSearchIndexFailure, FileSearchIndexOutcome, FileSearchIndexProgress, FileSearchIndexStatus,
    FileSearchMatch, FileSearchOutcome, MediaExifField, MediaSearchMetadata,
};
use crate::service::{
    BuildSelectedPathsRequest, IndexServiceCommand, IndexServiceEvent, SearchQuery,
};

use super::{
    IndexRequest, IndexRequestCommand, IndexResponse, WireBuildSelectedPathsRequest,
    WireDirectoryErrorPolicy, WireFileKind, WireFileSearchIndexFailure, WireFileSearchIndexOutcome,
    WireFileSearchIndexStatus, WireFileSearchMatch, WireFileSearchOutcome, WireIndexProfile,
    WireIndexServiceEvent, WireMediaExifField, WireMediaMetadataPolicy, WireMediaMetadataScope,
    WireMediaSearchKind, WireMediaSearchMetadata, WireOsString, WirePath, WireScanWarning,
    WireSearchMode, WireSearchQuery, WireSearchResultSource, INDEX_PROTOCOL_VERSION,
};

impl IndexRequest {
    pub fn from_command(index_base_dir: impl AsRef<Path>, command: IndexServiceCommand) -> Self {
        Self {
            version: INDEX_PROTOCOL_VERSION,
            index_base_dir: WirePath::from_path(index_base_dir.as_ref()),
            command: IndexRequestCommand::from(command),
        }
    }

    pub fn index_base_dir(&self) -> PathBuf {
        self.index_base_dir.to_path_buf()
    }
}

impl IndexRequestCommand {
    pub fn into_service_command(self) -> Option<IndexServiceCommand> {
        Some(match self {
            Self::Ping => IndexServiceCommand::Ping,
            Self::Shutdown => IndexServiceCommand::Shutdown,
            Self::ConfigureProfile(profile) => {
                IndexServiceCommand::ConfigureProfile(profile.into_domain())
            }
            Self::LoadProfile(profile_id) => IndexServiceCommand::LoadProfile(profile_id),
            Self::Query(query) => IndexServiceCommand::Query(query.into_domain()),
            Self::Rebuild { profile_id, root } => IndexServiceCommand::Rebuild {
                profile_id,
                root: root.to_path_buf(),
            },
            Self::BuildSelectedPaths(request) => {
                IndexServiceCommand::BuildSelectedPaths(request.into_domain())
            }
            Self::Status { profile_id, root } => IndexServiceCommand::Status {
                profile_id,
                root: root.to_path_buf(),
            },
            Self::ClearFailures { profile_id, root } => IndexServiceCommand::ClearFailures {
                profile_id,
                root: root.to_path_buf(),
            },
            Self::RemoveRoot { profile_id, root } => IndexServiceCommand::RemoveRoot {
                profile_id,
                root: root.to_path_buf(),
            },
            Self::DeleteProfile(profile_id) => IndexServiceCommand::DeleteProfile(profile_id),
        })
    }
}

impl From<IndexServiceCommand> for IndexRequestCommand {
    fn from(command: IndexServiceCommand) -> Self {
        match command {
            IndexServiceCommand::Ping => Self::Ping,
            IndexServiceCommand::Shutdown => Self::Shutdown,
            IndexServiceCommand::ConfigureProfile(profile) => {
                Self::ConfigureProfile(WireIndexProfile::from_domain(&profile))
            }
            IndexServiceCommand::LoadProfile(profile_id) => Self::LoadProfile(profile_id),
            IndexServiceCommand::Query(query) => Self::Query(WireSearchQuery::from_domain(&query)),
            IndexServiceCommand::Rebuild { profile_id, root } => Self::Rebuild {
                profile_id,
                root: WirePath::from_path(&root),
            },
            IndexServiceCommand::BuildSelectedPaths(request) => {
                Self::BuildSelectedPaths(WireBuildSelectedPathsRequest::from_domain(&request))
            }
            IndexServiceCommand::Status { profile_id, root } => Self::Status {
                profile_id,
                root: WirePath::from_path(&root),
            },
            IndexServiceCommand::ClearFailures { profile_id, root } => Self::ClearFailures {
                profile_id,
                root: WirePath::from_path(&root),
            },
            IndexServiceCommand::RemoveRoot { profile_id, root } => Self::RemoveRoot {
                profile_id,
                root: WirePath::from_path(&root),
            },
            IndexServiceCommand::DeleteProfile(profile_id) => Self::DeleteProfile(profile_id),
        }
    }
}

impl IndexResponse {
    pub fn from_event(event: &IndexServiceEvent) -> Self {
        Self::Event(WireIndexServiceEvent::from_domain(event))
    }

    pub fn from_progress(progress: FileSearchIndexProgress) -> Self {
        Self::Progress(progress.into())
    }
}

impl WirePath {
    pub fn from_path(path: &Path) -> Self {
        Self {
            bytes: path_to_bytes(path),
        }
    }

    pub fn to_path_buf(&self) -> PathBuf {
        path_from_bytes(self.bytes.clone())
    }
}

impl WireOsString {
    fn from_os_string(value: &OsString) -> Self {
        Self {
            bytes: os_string_to_bytes(value),
        }
    }

    fn to_os_string(&self) -> OsString {
        os_string_from_bytes(self.bytes.clone())
    }
}

impl WireIndexProfile {
    fn from_domain(profile: &IndexProfile) -> Self {
        Self {
            id: profile.id.clone(),
            roots: profile
                .roots
                .iter()
                .map(|root| WirePath::from_path(root))
                .collect(),
            include_hidden: profile.include_hidden,
            exclude_patterns: profile.exclude_patterns.clone(),
            directory_error_policy: WireDirectoryErrorPolicy::from(profile.directory_error_policy),
            media: WireMediaMetadataPolicy {
                scope: WireMediaMetadataScope::from(profile.media.scope),
            },
        }
    }

    fn into_domain(self) -> IndexProfile {
        IndexProfile {
            id: self.id,
            roots: self
                .roots
                .into_iter()
                .map(|root| root.to_path_buf())
                .collect(),
            include_hidden: self.include_hidden,
            exclude_patterns: self.exclude_patterns,
            directory_error_policy: self.directory_error_policy.into(),
            media: MediaMetadataPolicy {
                scope: self.media.scope.into(),
            },
        }
    }
}

impl WireSearchQuery {
    fn from_domain(query: &SearchQuery) -> Self {
        Self {
            profile_id: query.profile_id.clone(),
            root: WirePath::from_path(&query.root),
            text: query.text.clone(),
            mode: WireSearchMode::from(query.mode),
            limit: query.limit,
        }
    }

    fn into_domain(self) -> SearchQuery {
        SearchQuery {
            profile_id: self.profile_id,
            root: self.root.to_path_buf(),
            text: self.text,
            mode: self.mode.into(),
            limit: self.limit,
        }
    }
}

impl WireBuildSelectedPathsRequest {
    fn from_domain(request: &BuildSelectedPathsRequest) -> Self {
        Self {
            profile_id: request.profile_id.clone(),
            root: WirePath::from_path(&request.root),
            selected_paths: request
                .selected_paths
                .iter()
                .map(|path| WirePath::from_path(path))
                .collect(),
        }
    }

    pub fn into_domain(self) -> BuildSelectedPathsRequest {
        BuildSelectedPathsRequest {
            profile_id: self.profile_id,
            root: self.root.to_path_buf(),
            selected_paths: self
                .selected_paths
                .into_iter()
                .map(|path| path.to_path_buf())
                .collect(),
        }
    }
}

impl WireIndexServiceEvent {
    pub fn from_domain(event: &IndexServiceEvent) -> Self {
        match event {
            IndexServiceEvent::Pong { daemon_version } => Self::Pong {
                daemon_version: daemon_version.clone(),
            },
            IndexServiceEvent::Shutdown => Self::Shutdown,
            IndexServiceEvent::ProfileConfigured(id) => Self::ProfileConfigured(id.clone()),
            IndexServiceEvent::ProfileLoaded(profile) => {
                Self::ProfileLoaded(profile.as_ref().map(WireIndexProfile::from_domain))
            }
            IndexServiceEvent::QueryFinished(outcome) => {
                Self::QueryFinished(WireFileSearchOutcome::from_domain(outcome))
            }
            IndexServiceEvent::RebuildFinished(outcome) => {
                Self::RebuildFinished(WireFileSearchIndexOutcome::from_domain(outcome))
            }
            IndexServiceEvent::StatusLoaded(status) => {
                Self::StatusLoaded(WireFileSearchIndexStatus::from_domain(status))
            }
            IndexServiceEvent::FailuresCleared(status) => {
                Self::FailuresCleared(WireFileSearchIndexStatus::from_domain(status))
            }
            IndexServiceEvent::RootRemoved(status) => {
                Self::RootRemoved(WireFileSearchIndexStatus::from_domain(status))
            }
            IndexServiceEvent::ProfileDeleted(id) => Self::ProfileDeleted(id.clone()),
        }
    }

    pub fn into_domain(self) -> IndexServiceEvent {
        match self {
            Self::Pong { daemon_version } => IndexServiceEvent::Pong { daemon_version },
            Self::Shutdown => IndexServiceEvent::Shutdown,
            Self::ProfileConfigured(id) => IndexServiceEvent::ProfileConfigured(id),
            Self::ProfileLoaded(profile) => {
                IndexServiceEvent::ProfileLoaded(profile.map(WireIndexProfile::into_domain))
            }
            Self::QueryFinished(outcome) => IndexServiceEvent::QueryFinished(outcome.into_domain()),
            Self::RebuildFinished(outcome) => {
                IndexServiceEvent::RebuildFinished(outcome.into_domain())
            }
            Self::StatusLoaded(status) => IndexServiceEvent::StatusLoaded(status.into_domain()),
            Self::FailuresCleared(status) => {
                IndexServiceEvent::FailuresCleared(status.into_domain())
            }
            Self::RootRemoved(status) => IndexServiceEvent::RootRemoved(status.into_domain()),
            Self::ProfileDeleted(id) => IndexServiceEvent::ProfileDeleted(id),
        }
    }
}

impl WireFileSearchOutcome {
    fn from_domain(outcome: &FileSearchOutcome) -> Self {
        Self {
            root: WirePath::from_path(&outcome.root),
            matches: outcome
                .matches
                .iter()
                .map(WireFileSearchMatch::from_domain)
                .collect(),
            skipped: outcome
                .skipped
                .iter()
                .map(WireScanWarning::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> FileSearchOutcome {
        FileSearchOutcome {
            root: self.root.to_path_buf(),
            matches: self
                .matches
                .into_iter()
                .map(WireFileSearchMatch::into_domain)
                .collect(),
            skipped: self
                .skipped
                .into_iter()
                .map(WireScanWarning::into_domain)
                .collect(),
        }
    }
}

impl WireFileSearchMatch {
    fn from_domain(search_match: &FileSearchMatch) -> Self {
        Self {
            path: WirePath::from_path(&search_match.path),
            relative_path: WirePath::from_path(&search_match.relative_path),
            name: WireOsString::from_os_string(&search_match.name),
            kind: WireFileKind::from(search_match.kind),
            rank_score: search_match.rank_score,
            source: WireSearchResultSource::from(search_match.source),
            snippet: search_match.snippet.clone(),
            media: search_match
                .media
                .as_ref()
                .map(WireMediaSearchMetadata::from_domain),
        }
    }

    fn into_domain(self) -> FileSearchMatch {
        FileSearchMatch {
            path: self.path.to_path_buf(),
            relative_path: self.relative_path.to_path_buf(),
            name: self.name.to_os_string(),
            kind: self.kind.into(),
            rank_score: self.rank_score,
            source: self.source.into(),
            snippet: self.snippet,
            media: self.media.map(WireMediaSearchMetadata::into_domain),
        }
    }
}

impl WireMediaSearchMetadata {
    fn from_domain(media: &MediaSearchMetadata) -> Self {
        Self {
            media_kind: WireMediaSearchKind::from(media.media_kind),
            width: media.width,
            height: media.height,
            duration_ms: media.duration_ms,
            codec: media.codec.clone(),
            title: media.title.clone(),
            artist: media.artist.clone(),
            exif: media
                .exif
                .iter()
                .map(WireMediaExifField::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> MediaSearchMetadata {
        MediaSearchMetadata {
            media_kind: self.media_kind.into(),
            width: self.width,
            height: self.height,
            duration_ms: self.duration_ms,
            codec: self.codec,
            title: self.title,
            artist: self.artist,
            exif: self
                .exif
                .into_iter()
                .map(WireMediaExifField::into_domain)
                .collect(),
        }
    }
}

impl WireMediaExifField {
    fn from_domain(field: &MediaExifField) -> Self {
        Self {
            tag: field.tag.clone(),
            value: field.value.clone(),
        }
    }

    fn into_domain(self) -> MediaExifField {
        MediaExifField {
            tag: self.tag,
            value: self.value,
        }
    }
}

impl WireFileSearchIndexOutcome {
    fn from_domain(outcome: &FileSearchIndexOutcome) -> Self {
        Self {
            root: WirePath::from_path(&outcome.root),
            index_dir: WirePath::from_path(&outcome.index_dir),
            indexed_count: outcome.indexed_count,
            index_size_bytes: outcome.index_size_bytes,
            updated_at_ms: outcome.updated_at_ms,
            failed_count: outcome.failed_count,
            skipped: outcome
                .skipped
                .iter()
                .map(WireScanWarning::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> FileSearchIndexOutcome {
        FileSearchIndexOutcome {
            root: self.root.to_path_buf(),
            index_dir: self.index_dir.to_path_buf(),
            indexed_count: self.indexed_count,
            index_size_bytes: self.index_size_bytes,
            updated_at_ms: self.updated_at_ms,
            failed_count: self.failed_count,
            skipped: self
                .skipped
                .into_iter()
                .map(WireScanWarning::into_domain)
                .collect(),
        }
    }
}

impl WireFileSearchIndexStatus {
    fn from_domain(status: &FileSearchIndexStatus) -> Self {
        Self {
            root: WirePath::from_path(&status.root),
            index_dir: WirePath::from_path(&status.index_dir),
            exists: status.exists,
            stale: status.stale,
            reason: status.reason.clone(),
            include_hidden: status.include_hidden,
            media_metadata_scope: WireMediaMetadataScope::from(status.media_metadata_scope),
            record_count: status.record_count,
            index_size_bytes: status.index_size_bytes,
            built_at_ms: status.built_at_ms,
            updated_at_ms: status.updated_at_ms,
            failed_count: status.failed_count,
            exclude_rules_hash: status.exclude_rules_hash.clone(),
            extractor_version: status.extractor_version,
            failures: status
                .failures
                .iter()
                .map(WireFileSearchIndexFailure::from_domain)
                .collect(),
        }
    }

    fn into_domain(self) -> FileSearchIndexStatus {
        FileSearchIndexStatus {
            root: self.root.to_path_buf(),
            index_dir: self.index_dir.to_path_buf(),
            exists: self.exists,
            stale: self.stale,
            reason: self.reason,
            include_hidden: self.include_hidden,
            media_metadata_scope: self.media_metadata_scope.into(),
            record_count: self.record_count,
            index_size_bytes: self.index_size_bytes,
            built_at_ms: self.built_at_ms,
            updated_at_ms: self.updated_at_ms,
            failed_count: self.failed_count,
            exclude_rules_hash: self.exclude_rules_hash,
            extractor_version: self.extractor_version,
            failures: self
                .failures
                .into_iter()
                .map(WireFileSearchIndexFailure::into_domain)
                .collect(),
        }
    }
}

impl WireFileSearchIndexFailure {
    fn from_domain(failure: &FileSearchIndexFailure) -> Self {
        Self {
            path: WirePath::from_path(&failure.path),
            message: failure.message.clone(),
            first_failed_at_ms: failure.first_failed_at_ms,
            last_failed_at_ms: failure.last_failed_at_ms,
            retry_count: failure.retry_count,
        }
    }

    fn into_domain(self) -> FileSearchIndexFailure {
        FileSearchIndexFailure {
            path: self.path.to_path_buf(),
            message: self.message,
            first_failed_at_ms: self.first_failed_at_ms,
            last_failed_at_ms: self.last_failed_at_ms,
            retry_count: self.retry_count,
        }
    }
}

impl WireScanWarning {
    fn from_domain(warning: &ScanWarning) -> Self {
        Self {
            path: WirePath::from_path(&warning.path),
            message: warning.message.clone(),
        }
    }

    fn into_domain(self) -> ScanWarning {
        ScanWarning {
            path: self.path.to_path_buf(),
            message: self.message,
        }
    }
}

#[cfg(unix)]
fn os_string_to_bytes(value: &OsString) -> Vec<u8> {
    value.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_string_to_bytes(value: &OsString) -> Vec<u8> {
    value.to_string_lossy().as_bytes().to_vec()
}

#[cfg(unix)]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from_vec(bytes)
}

#[cfg(not(unix))]
fn os_string_from_bytes(bytes: Vec<u8>) -> OsString {
    OsString::from(String::from_utf8_lossy(&bytes).into_owned())
}
