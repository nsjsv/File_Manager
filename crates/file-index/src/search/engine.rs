use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::ScanWarning;

use super::cache::SearchQueryRuntime;
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
    runtime: &SearchQueryRuntime,
    query: &str,
    options: &FileSearchOptions,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    match options.mode {
        SearchMode::Files => Ok(query::search_catalog(
            runtime.catalog(),
            query,
            options.limit.max(1),
        )),
        SearchMode::Contents => search_tantivy_source(
            runtime,
            query,
            &[SearchResultSource::Contents],
            options.content_index_enabled,
            options.limit,
        ),
        SearchMode::Media => search_tantivy_source(
            runtime,
            query,
            &[SearchResultSource::Media],
            options.media_metadata_scope.includes_media(),
            options.limit,
        ),
        SearchMode::All => {
            let mut matches = query::search_catalog(runtime.catalog(), query, options.limit.max(1));
            let mut sources = Vec::with_capacity(2);
            if options.content_index_enabled {
                sources.push(SearchResultSource::Contents);
            }
            if options.media_metadata_scope.includes_media() {
                sources.push(SearchResultSource::Media);
            }
            if !sources.is_empty() {
                matches.extend(search_tantivy_matches(
                    runtime,
                    query,
                    &sources,
                    options.limit,
                )?);
            }
            Ok(merge_search_matches(matches, options.limit.max(1)))
        }
    }
}

fn search_tantivy_source(
    runtime: &SearchQueryRuntime,
    query: &str,
    sources: &[SearchResultSource],
    enabled: bool,
    limit: usize,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    if !enabled {
        return Ok(Vec::new());
    }
    search_tantivy_matches(runtime, query, sources, limit)
}

fn search_tantivy_matches(
    runtime: &SearchQueryRuntime,
    query: &str,
    sources: &[SearchResultSource],
    limit: usize,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    let full_text_runtime = runtime.full_text_runtime()?;
    Ok(full_text_hits_to_matches(
        runtime.catalog(),
        search_tantivy_index(full_text_runtime.as_deref(), query, sources, limit)?,
    ))
}

fn full_text_hits_to_matches(
    catalog: &SearchCatalog,
    hits: Vec<FullTextSearchHit>,
) -> Vec<FileSearchMatch> {
    hits.into_iter()
        .filter_map(|hit| {
            let record = catalog.record_by_storage_key(&hit.storage_key)?;
            Some(record.to_search_match(hit.score, hit.source, hit.snippet, hit.media))
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
