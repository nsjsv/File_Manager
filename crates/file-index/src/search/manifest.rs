use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use super::ignore_policy::exclude_rules_hash;
use super::path_encoding::path_storage_key;
use super::search_index_error;
use super::types::{
    DirectoryErrorPolicy, FileSearchIndexFailure, FileSearchIndexStatus, EXTRACTOR_VERSION,
    IGNORE_POLICY_VERSION, INDEX_FORMAT_VERSION,
};
use crate::profile::MediaMetadataScope;
use crate::IndexError;

const MANIFEST_FORMAT_VERSION: &str = "format_version";
const MANIFEST_ROOT_KEY: &str = "root_key";
const MANIFEST_ROOT_TEXT: &str = "root_text";
const MANIFEST_INCLUDE_HIDDEN: &str = "include_hidden";
const MANIFEST_DIRECTORY_ERROR_POLICY: &str = "directory_error_policy";
const MANIFEST_IGNORE_POLICY_VERSION: &str = "ignore_policy_version";
const MANIFEST_RECORD_COUNT: &str = "record_count";
const MANIFEST_GENERATION: &str = "generation";
const MANIFEST_EXCLUDE_RULES_HASH: &str = "exclude_rules_hash";
const MANIFEST_BUILT_AT_MS: &str = "built_at_ms";
const MANIFEST_UPDATED_AT_MS: &str = "updated_at_ms";
pub(crate) const MANIFEST_INDEX_SIZE_BYTES: &str = "index_size_bytes";
pub(crate) const MANIFEST_FAILED_COUNT: &str = "failed_count";
const MANIFEST_EXTRACTOR_VERSION: &str = "extractor_version";
const MANIFEST_CONTENT_INDEX_ENABLED: &str = "content_index_enabled";
const MANIFEST_CONTENT_MAX_FILE_BYTES: &str = "content_max_file_bytes";
const MANIFEST_MEDIA_INDEX_ENABLED: &str = "media_index_enabled";
const MANIFEST_MEDIA_METADATA_SCOPE: &str = "media_metadata_scope";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchCatalogIdentity {
    pub(crate) generation: String,
    pub(crate) record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchIndexManifest {
    pub(crate) format_version: u32,
    pub(crate) root_key: String,
    pub(crate) root_text: String,
    pub(crate) include_hidden: bool,
    pub(crate) directory_error_policy: DirectoryErrorPolicy,
    pub(crate) ignore_policy_version: u32,
    pub(crate) record_count: usize,
    pub(crate) generation: String,
    pub(crate) exclude_rules_hash: String,
    pub(crate) built_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) index_size_bytes: u64,
    pub(crate) failed_count: usize,
    pub(crate) extractor_version: u32,
    pub(crate) content_index_enabled: bool,
    pub(crate) content_max_file_bytes: u64,
    pub(crate) media_metadata_scope: MediaMetadataScope,
}

impl SearchIndexManifest {
    pub(crate) fn new(
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
        directory_error_policy: DirectoryErrorPolicy,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
        record_count: usize,
        failed_count: usize,
        built_at_ms: Option<i64>,
    ) -> Self {
        let updated_at_ms = current_time_ms();
        let built_at_ms = built_at_ms.unwrap_or(updated_at_ms);
        let root_key = path_storage_key(root);
        let exclude_rules_hash = exclude_rules_hash(exclude_patterns);
        let generation = format!(
            "{updated_at_ms}:{record_count}:{failed_count}:{root_key}:{include_hidden}:{exclude_rules_hash}"
        );

        Self {
            format_version: INDEX_FORMAT_VERSION,
            root_key,
            root_text: root.to_string_lossy().into_owned(),
            include_hidden,
            directory_error_policy,
            ignore_policy_version: IGNORE_POLICY_VERSION,
            record_count,
            generation,
            exclude_rules_hash,
            built_at_ms,
            updated_at_ms,
            index_size_bytes: 0,
            failed_count,
            extractor_version: EXTRACTOR_VERSION,
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
        }
    }

    pub(crate) fn identity(&self) -> SearchCatalogIdentity {
        SearchCatalogIdentity {
            generation: self.generation.clone(),
            record_count: self.record_count,
        }
    }

    pub(crate) fn validate_for(
        &self,
        index_dir: &Path,
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
        directory_error_policy: DirectoryErrorPolicy,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
    ) -> Result<(), IndexError> {
        if let Some(reason) = self.stale_reason_for(
            root,
            include_hidden,
            exclude_patterns,
            directory_error_policy,
            content_index_enabled,
            content_max_file_bytes,
            media_metadata_scope,
        ) {
            return Err(search_index_error(index_dir, reason));
        }
        Ok(())
    }

    pub(crate) fn stale_reason_for(
        &self,
        root: &Path,
        include_hidden: bool,
        exclude_patterns: &[String],
        directory_error_policy: DirectoryErrorPolicy,
        content_index_enabled: bool,
        content_max_file_bytes: u64,
        media_metadata_scope: MediaMetadataScope,
    ) -> Option<String> {
        if self.format_version != INDEX_FORMAT_VERSION {
            return Some("search index format is outdated".to_owned());
        }
        if self.ignore_policy_version != IGNORE_POLICY_VERSION {
            return Some("search index ignore policy is outdated".to_owned());
        }
        if self.extractor_version != EXTRACTOR_VERSION {
            return Some("search index extractor version is outdated".to_owned());
        }
        if self.root_key != path_storage_key(root) {
            return Some("search index root does not match".to_owned());
        }
        if self.include_hidden != include_hidden {
            return Some(
                "search index options do not match current hidden-file setting".to_owned(),
            );
        }
        if self.exclude_rules_hash != exclude_rules_hash(exclude_patterns) {
            return Some("search index exclude rules are outdated".to_owned());
        }
        if self.directory_error_policy != directory_error_policy {
            return Some("search index directory error policy is outdated".to_owned());
        }
        if self.content_index_enabled != content_index_enabled {
            return Some("search index content policy is outdated".to_owned());
        }
        if self.content_max_file_bytes != content_max_file_bytes {
            return Some("search index content size policy is outdated".to_owned());
        }
        if self.media_metadata_scope != media_metadata_scope {
            return Some("search index media policy is outdated".to_owned());
        }
        None
    }

    pub(crate) fn to_status(
        &self,
        root: PathBuf,
        index_dir: PathBuf,
        failures: Vec<FileSearchIndexFailure>,
        index_size_bytes: u64,
    ) -> FileSearchIndexStatus {
        FileSearchIndexStatus {
            root,
            index_dir,
            exists: true,
            stale: false,
            reason: None,
            include_hidden: self.include_hidden,
            content_index_enabled: self.content_index_enabled,
            content_max_file_bytes: self.content_max_file_bytes,
            media_metadata_scope: self.media_metadata_scope,
            record_count: self.record_count,
            index_size_bytes,
            built_at_ms: Some(self.built_at_ms),
            updated_at_ms: Some(self.updated_at_ms),
            failed_count: failures.len().max(self.failed_count),
            exclude_rules_hash: Some(self.exclude_rules_hash.clone()),
            extractor_version: Some(self.extractor_version),
            failures,
        }
    }

    pub(crate) fn entries(&self) -> Vec<(&'static str, String)> {
        vec![
            (MANIFEST_FORMAT_VERSION, self.format_version.to_string()),
            (MANIFEST_ROOT_KEY, self.root_key.clone()),
            (MANIFEST_ROOT_TEXT, self.root_text.clone()),
            (MANIFEST_INCLUDE_HIDDEN, self.include_hidden.to_string()),
            (
                MANIFEST_DIRECTORY_ERROR_POLICY,
                self.directory_error_policy.config_value().to_string(),
            ),
            (
                MANIFEST_IGNORE_POLICY_VERSION,
                self.ignore_policy_version.to_string(),
            ),
            (MANIFEST_RECORD_COUNT, self.record_count.to_string()),
            (MANIFEST_GENERATION, self.generation.clone()),
            (MANIFEST_EXCLUDE_RULES_HASH, self.exclude_rules_hash.clone()),
            (MANIFEST_BUILT_AT_MS, self.built_at_ms.to_string()),
            (MANIFEST_UPDATED_AT_MS, self.updated_at_ms.to_string()),
            (MANIFEST_INDEX_SIZE_BYTES, self.index_size_bytes.to_string()),
            (MANIFEST_FAILED_COUNT, self.failed_count.to_string()),
            (
                MANIFEST_EXTRACTOR_VERSION,
                self.extractor_version.to_string(),
            ),
            (
                MANIFEST_CONTENT_INDEX_ENABLED,
                self.content_index_enabled.to_string(),
            ),
            (
                MANIFEST_CONTENT_MAX_FILE_BYTES,
                self.content_max_file_bytes.to_string(),
            ),
            (
                MANIFEST_MEDIA_METADATA_SCOPE,
                self.media_metadata_scope.config_value().to_string(),
            ),
        ]
    }
}

pub(crate) fn read_manifest_from_connection(
    index_dir: &Path,
    connection: &Connection,
) -> Result<SearchIndexManifest, IndexError> {
    let mut statement = connection
        .prepare("SELECT key, value FROM manifest")
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut rows = statement
        .query([])
        .map_err(|error| search_index_error(index_dir, error))?;
    let mut values = HashMap::new();

    while let Some(row) = rows
        .next()
        .map_err(|error| search_index_error(index_dir, error))?
    {
        let key = row
            .get::<_, String>(0)
            .map_err(|error| search_index_error(index_dir, error))?;
        let value = row
            .get::<_, String>(1)
            .map_err(|error| search_index_error(index_dir, error))?;
        values.insert(key, value);
    }

    let format_version = parse_manifest_u32(index_dir, &values, MANIFEST_FORMAT_VERSION)?;

    Ok(SearchIndexManifest {
        format_version,
        root_key: required_manifest_value(index_dir, &values, MANIFEST_ROOT_KEY)?,
        root_text: required_manifest_value(index_dir, &values, MANIFEST_ROOT_TEXT)?,
        include_hidden: parse_manifest_bool(index_dir, &values, MANIFEST_INCLUDE_HIDDEN)?,
        directory_error_policy: optional_manifest_directory_error_policy(
            index_dir,
            &values,
            MANIFEST_DIRECTORY_ERROR_POLICY,
        )?
        .unwrap_or_default(),
        ignore_policy_version: parse_manifest_u32(
            index_dir,
            &values,
            MANIFEST_IGNORE_POLICY_VERSION,
        )?,
        record_count: parse_manifest_usize(index_dir, &values, MANIFEST_RECORD_COUNT)?,
        generation: required_manifest_value(index_dir, &values, MANIFEST_GENERATION)?,
        exclude_rules_hash: required_manifest_value(
            index_dir,
            &values,
            MANIFEST_EXCLUDE_RULES_HASH,
        )?,
        built_at_ms: parse_manifest_i64(index_dir, &values, MANIFEST_BUILT_AT_MS)?,
        updated_at_ms: parse_manifest_i64(index_dir, &values, MANIFEST_UPDATED_AT_MS)?,
        index_size_bytes: parse_manifest_u64(index_dir, &values, MANIFEST_INDEX_SIZE_BYTES)?,
        failed_count: parse_manifest_usize(index_dir, &values, MANIFEST_FAILED_COUNT)?,
        extractor_version: parse_manifest_u32(index_dir, &values, MANIFEST_EXTRACTOR_VERSION)?,
        content_index_enabled: optional_manifest_bool(
            index_dir,
            &values,
            MANIFEST_CONTENT_INDEX_ENABLED,
        )?
        .unwrap_or(false),
        content_max_file_bytes: optional_manifest_u64(
            index_dir,
            &values,
            MANIFEST_CONTENT_MAX_FILE_BYTES,
        )?
        .unwrap_or(16 * 1024 * 1024),
        media_metadata_scope: optional_manifest_media_metadata_scope(index_dir, &values)?
            .unwrap_or(MediaMetadataScope::Off),
    })
}

pub(crate) fn current_time_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn required_manifest_value(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<String, IndexError> {
    values
        .get(key)
        .cloned()
        .ok_or_else(|| search_index_error(index_dir, format!("search manifest is missing {key}")))
}

fn parse_manifest_u32(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<u32, IndexError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_i64(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<i64, IndexError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_u64(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<u64, IndexError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn optional_manifest_u64(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<Option<u64>, IndexError> {
    values
        .get(key)
        .map(|value| {
            value
                .parse()
                .map_err(|error| search_index_error(index_dir, error))
        })
        .transpose()
}

fn parse_manifest_usize(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<usize, IndexError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn parse_manifest_bool(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<bool, IndexError> {
    required_manifest_value(index_dir, values, key)?
        .parse()
        .map_err(|error| search_index_error(index_dir, error))
}

fn optional_manifest_bool(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<Option<bool>, IndexError> {
    values
        .get(key)
        .map(|value| {
            value
                .parse()
                .map_err(|error| search_index_error(index_dir, error))
        })
        .transpose()
}

fn optional_manifest_media_metadata_scope(
    index_dir: &Path,
    values: &HashMap<String, String>,
) -> Result<Option<MediaMetadataScope>, IndexError> {
    if let Some(value) = values.get(MANIFEST_MEDIA_METADATA_SCOPE) {
        return MediaMetadataScope::from_config_value(value)
            .ok_or_else(|| {
                search_index_error(index_dir, format!("unknown media metadata scope {value}"))
            })
            .map(Some);
    }

    optional_manifest_bool(index_dir, values, MANIFEST_MEDIA_INDEX_ENABLED).map(|legacy| {
        legacy.map(|enabled| {
            if enabled {
                MediaMetadataScope::All
            } else {
                MediaMetadataScope::Off
            }
        })
    })
}

fn optional_manifest_directory_error_policy(
    index_dir: &Path,
    values: &HashMap<String, String>,
    key: &str,
) -> Result<Option<DirectoryErrorPolicy>, IndexError> {
    values
        .get(key)
        .map(|value| {
            DirectoryErrorPolicy::from_config_value(value).ok_or_else(|| {
                search_index_error(index_dir, format!("unknown directory error policy {value}"))
            })
        })
        .transpose()
}
