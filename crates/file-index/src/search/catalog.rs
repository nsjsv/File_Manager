use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nucleo::Utf32String;

use super::manifest::{SearchCatalogIdentity, SearchIndexManifest};
use super::path_encoding::path_storage_key;
use super::types::{
    FileSearchMatch, MediaSearchMetadata, SearchIndexFileRecord, SearchResultSource,
};
use file_core::FileKind;

#[derive(Clone)]
pub(crate) struct SearchCatalog {
    records: Vec<SearchCatalogRecord>,
    trigram_index: HashMap<String, Vec<usize>>,
    record_lookup: HashMap<String, usize>,
    identity: Option<SearchCatalogIdentity>,
}

#[derive(Clone)]
pub(crate) struct SearchCatalogRecord {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    pub(crate) name: OsString,
    pub(crate) kind: FileKind,
    pub(crate) path_text: String,
    pub(crate) normalized_name: String,
    pub(crate) normalized_path: String,
    pub(crate) name_utf32: Utf32String,
    pub(crate) path_utf32: Utf32String,
    pub(crate) storage_key: String,
    pub(crate) mtime_ms: Option<i64>,
    pub(crate) size_bytes: Option<u64>,
}

impl SearchCatalog {
    pub(crate) fn from_records(
        _root: PathBuf,
        records: Vec<SearchCatalogRecord>,
        manifest: Option<&SearchIndexManifest>,
    ) -> Self {
        let mut trigram_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut record_lookup = HashMap::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            for trigram in record.search_trigrams() {
                trigram_index.entry(trigram).or_default().push(index);
            }
            record_lookup.insert(record.storage_key.clone(), index);
        }

        Self {
            records,
            trigram_index,
            record_lookup,
            identity: manifest.map(SearchIndexManifest::identity),
        }
    }

    pub(crate) fn records(&self) -> &[SearchCatalogRecord] {
        &self.records
    }

    pub(crate) fn identity(&self) -> Option<&SearchCatalogIdentity> {
        self.identity.as_ref()
    }

    pub(crate) fn record_by_storage_key(&self, storage_key: &str) -> Option<&SearchCatalogRecord> {
        let index = self.record_lookup.get(storage_key)?;
        self.records.get(*index)
    }

    pub(crate) fn trigram_candidates(&self, query: &str) -> Vec<usize> {
        let trigrams = unique_trigrams(query);
        let Some((first, rest)) = trigrams.split_first() else {
            return Vec::new();
        };
        let Some(first_matches) = self.trigram_index.get(first) else {
            return Vec::new();
        };
        if rest.is_empty() {
            return first_matches.clone();
        }

        first_matches
            .iter()
            .copied()
            .filter(|index| {
                rest.iter().all(|trigram| {
                    self.trigram_index
                        .get(trigram)
                        .is_some_and(|matches| matches.binary_search(index).is_ok())
                })
            })
            .collect()
    }
}

impl SearchCatalogRecord {
    pub(crate) fn from_path_with_metadata(
        root: &Path,
        path: PathBuf,
        kind: FileKind,
        metadata: &std::fs::Metadata,
    ) -> Self {
        Self::from_path_with_index_metadata(
            root,
            path,
            kind,
            metadata_mtime_ms(metadata),
            Some(metadata.len()),
        )
    }

    pub(crate) fn from_path_with_index_metadata(
        root: &Path,
        path: PathBuf,
        kind: FileKind,
        mtime_ms: Option<i64>,
        size_bytes: Option<u64>,
    ) -> Self {
        let relative_path = path
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.clone());
        let name = path
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| path.as_os_str().to_os_string());
        let name_text = name.to_string_lossy().into_owned();
        let path_text = relative_path.to_string_lossy().into_owned();
        let normalized_name = normalize_search_text(&name_text);
        let normalized_path = normalize_search_text(&path_text);
        let name_utf32 = Utf32String::from(name_text.as_str());
        let path_utf32 = Utf32String::from(path_text.as_str());
        let storage_key = path_storage_key(&path);

        Self {
            path,
            relative_path,
            name,
            kind,
            path_text,
            normalized_name,
            normalized_path,
            name_utf32,
            path_utf32,
            storage_key,
            mtime_ms,
            size_bytes,
        }
    }

    pub(crate) fn to_match(&self, rank_score: u32) -> FileSearchMatch {
        self.to_search_match(rank_score, SearchResultSource::Files, None, None)
    }

    pub(crate) fn to_search_match(
        &self,
        rank_score: u32,
        source: SearchResultSource,
        snippet: Option<String>,
        media: Option<MediaSearchMetadata>,
    ) -> FileSearchMatch {
        FileSearchMatch {
            path: self.path.clone(),
            relative_path: self.relative_path.clone(),
            name: self.name.clone(),
            kind: self.kind,
            rank_score,
            source,
            snippet,
            media,
        }
    }

    pub(crate) fn to_file_record(&self) -> SearchIndexFileRecord {
        SearchIndexFileRecord {
            path: self.path.clone(),
            relative_path: self.relative_path.clone(),
            kind: self.kind,
            mtime_ms: self.mtime_ms,
            size_bytes: self.size_bytes,
        }
    }

    pub(crate) fn segment_starts_with(&self, query: &str) -> bool {
        self.normalized_path
            .split(['/', '\\', ' ', '-', '_', '.'])
            .any(|segment| segment.starts_with(query))
    }

    fn search_trigrams(&self) -> Vec<String> {
        let combined = format!("{}\n{}", self.normalized_name, self.normalized_path);
        unique_trigrams(&combined)
    }
}

fn metadata_mtime_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    let modified = metadata.modified().ok()?;
    let millis = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().min(i64::MAX as u128) as i64,
        Err(error) => -(error.duration().as_millis().min(i64::MAX as u128) as i64),
    };
    Some(millis)
}

pub(crate) fn normalize_search_text(text: &str) -> String {
    text.to_lowercase()
}

fn unique_trigrams(text: &str) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() < 3 {
        return Vec::new();
    }

    let mut trigrams = Vec::new();
    for window in chars.windows(3) {
        let trigram = window.iter().collect::<String>();
        if !trigrams.contains(&trigram) {
            trigrams.push(trigram);
        }
    }
    trigrams.sort_unstable();
    trigrams
}
