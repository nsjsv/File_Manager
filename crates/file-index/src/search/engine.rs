use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use file_core::ScanWarning;

use super::catalog::{SearchCatalog, SearchCatalogRecord};
use super::extractor::{extract_media_documents, extract_text_documents};
use super::full_text::{search_tantivy_index, write_tantivy_index, FullTextSearchHit};
use super::query;
use super::types::{FileSearchMatch, FileSearchOptions, SearchResultSource};
use crate::profile::MediaMetadataScope;
use crate::{IndexError, SearchMode};

pub(crate) fn write_search_documents(
    pending_index_dir: &Path,
    records: &[SearchCatalogRecord],
    content_index_enabled: bool,
    content_max_file_bytes: u64,
    media_metadata_scope: MediaMetadataScope,
) -> Result<Vec<ScanWarning>, IndexError> {
    let mut warnings = Vec::new();
    let text_documents = if content_index_enabled {
        extract_text_documents(records, content_max_file_bytes)
    } else {
        (Vec::new(), Vec::new())
    };
    let (text_documents, text_warnings) = text_documents;
    warnings.extend(text_warnings);

    let media_documents = if media_metadata_scope.includes_media() {
        extract_media_documents(records, media_metadata_scope)
    } else {
        (Vec::new(), Vec::new())
    };
    let (media_documents, media_warnings) = media_documents;
    warnings.extend(media_warnings);

    write_tantivy_index(pending_index_dir, &text_documents, &media_documents)?;
    Ok(warnings)
}

pub(crate) fn search_index_catalog_and_tantivy(
    index_dir: &Path,
    catalog: &SearchCatalog,
    query: &str,
    options: &FileSearchOptions,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    match options.mode {
        SearchMode::Files => Ok(query::search_catalog(catalog, query, options.limit.max(1))),
        SearchMode::Contents => search_tantivy_source(
            index_dir,
            query,
            SearchResultSource::Contents,
            options.content_index_enabled,
            options.limit,
        ),
        SearchMode::Media => search_tantivy_source(
            index_dir,
            query,
            SearchResultSource::Media,
            options.media_metadata_scope.includes_media(),
            options.limit,
        ),
        SearchMode::All => {
            let mut matches = query::search_catalog(catalog, query, options.limit.max(1));
            if options.content_index_enabled {
                matches.extend(full_text_hits_to_matches(search_tantivy_index(
                    index_dir,
                    query,
                    &[SearchResultSource::Contents],
                    options.limit,
                )?));
            }
            if options.media_metadata_scope.includes_media() {
                matches.extend(full_text_hits_to_matches(search_tantivy_index(
                    index_dir,
                    query,
                    &[SearchResultSource::Media],
                    options.limit,
                )?));
            }
            Ok(merge_search_matches(matches, options.limit.max(1)))
        }
    }
}

fn search_tantivy_source(
    index_dir: &Path,
    query: &str,
    source: SearchResultSource,
    enabled: bool,
    limit: usize,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    if !enabled {
        return Ok(Vec::new());
    }
    Ok(full_text_hits_to_matches(search_tantivy_index(
        index_dir,
        query,
        &[source],
        limit,
    )?))
}

fn full_text_hits_to_matches(hits: Vec<FullTextSearchHit>) -> Vec<FileSearchMatch> {
    hits.into_iter()
        .map(|hit| {
            let kind = hit
                .path
                .symlink_metadata()
                .ok()
                .map(|metadata| {
                    let file_type = metadata.file_type();
                    if file_type.is_dir() {
                        file_core::FileKind::Directory
                    } else if file_type.is_file() {
                        file_core::FileKind::File
                    } else if file_type.is_symlink() {
                        file_core::FileKind::Symlink
                    } else {
                        file_core::FileKind::Other
                    }
                })
                .unwrap_or(file_core::FileKind::File);
            FileSearchMatch {
                path: hit.path,
                relative_path: hit.relative_path,
                name: OsString::from(hit.name),
                kind,
                rank_score: hit.score,
                source: hit.source,
                snippet: hit.snippet,
                media: hit.media,
            }
        })
        .collect()
}

fn merge_search_matches(matches: Vec<FileSearchMatch>, limit: usize) -> Vec<FileSearchMatch> {
    let mut by_path = HashMap::<PathBuf, FileSearchMatch>::new();
    for search_match in matches {
        by_path
            .entry(search_match.path.clone())
            .and_modify(|existing| {
                if search_match.rank_score > existing.rank_score
                    || existing.source == SearchResultSource::Files
                {
                    *existing = search_match.clone();
                }
            })
            .or_insert(search_match);
    }

    let mut merged = by_path.into_values().collect::<Vec<_>>();
    merged.sort_by(|left, right| {
        right
            .rank_score
            .cmp(&left.rank_score)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    merged.truncate(limit);
    merged
}
