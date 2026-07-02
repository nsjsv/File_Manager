use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_core::ScanWarning;

use super::cache::SearchQueryRuntime;
use super::catalog::SearchCatalogRecord;
use super::extractor::{extract_media_documents, extract_text_documents};
use super::full_text::{search_tantivy_index, write_tantivy_index, FullTextSearchHit};
use super::types::{FileSearchMatch, FileSearchOptions, SearchResultSource};
use super::{query, store};
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
        SearchMode::Files => search_catalog_records(runtime, query, options),
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
            let mut matches = search_catalog_records(runtime, query, options)?;
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

fn search_catalog_records(
    runtime: &SearchQueryRuntime,
    query_text: &str,
    options: &FileSearchOptions,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    let mut collector = query::SearchMatchCollector::new(query_text, options.limit.max(1));
    store::scan_catalog_records(
        runtime.index_dir(),
        runtime.root(),
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.content_index_enabled,
        options.content_max_file_bytes,
        options.media_metadata_scope,
        |record| {
            collector.push_record(&record);
            Ok(())
        },
    )?;
    Ok(collector.finish())
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
    full_text_hits_to_matches(
        runtime,
        search_tantivy_index(full_text_runtime.as_deref(), query, sources, limit)?,
    )
}

fn full_text_hits_to_matches(
    runtime: &SearchQueryRuntime,
    hits: Vec<FullTextSearchHit>,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    let mut matches = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(record) = store::read_catalog_record_by_storage_key(
            runtime.index_dir(),
            runtime.root(),
            &hit.storage_key,
        )?
        else {
            continue;
        };
        matches.push(record.to_search_match(hit.score, hit.source, hit.snippet, hit.media));
    }
    Ok(matches)
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
