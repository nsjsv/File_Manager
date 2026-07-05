use std::collections::HashMap;
use std::path::PathBuf;

use super::cache::SearchQueryRuntime;
use super::full_text::{search_tantivy_index, FullTextSearchHit};
use super::types::{FileSearchMatch, FileSearchOptions, SearchResultSource};
use super::{query, store};
use crate::{IndexError, SearchMode};
use tokio_util::sync::CancellationToken;

pub(crate) fn search_index_catalog_and_tantivy(
    runtime: &SearchQueryRuntime,
    query: &str,
    options: &FileSearchOptions,
    cancel: &CancellationToken,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    ensure_not_cancelled(cancel)?;
    match options.mode {
        SearchMode::Files => search_catalog_records(runtime, query, options, cancel),
        SearchMode::Contents => Err(IndexError::store(
            runtime.index_dir(),
            "contents search is routed through rg",
        )),
        SearchMode::Media => search_tantivy_source(
            runtime,
            query,
            &[SearchResultSource::Media],
            options.media_metadata_scope.includes_media(),
            options.limit,
            cancel,
        ),
        SearchMode::All => {
            let mut matches = search_catalog_records(runtime, query, options, cancel)?;
            if options.media_metadata_scope.includes_media() {
                matches.extend(search_tantivy_matches(
                    runtime,
                    query,
                    &[SearchResultSource::Media],
                    options.limit,
                    cancel,
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
    cancel: &CancellationToken,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    let mut collector = query::SearchMatchCollector::new(query_text, options.limit.max(1));
    store::scan_file_query_candidates_with_cancel(
        runtime.index_dir(),
        runtime.root(),
        options.include_hidden,
        &options.exclude_patterns,
        options.directory_error_policy,
        options.media_metadata_scope,
        query_text,
        options.limit.max(1),
        cancel,
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
    cancel: &CancellationToken,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    if !enabled {
        return Ok(Vec::new());
    }
    search_tantivy_matches(runtime, query, sources, limit, cancel)
}

fn search_tantivy_matches(
    runtime: &SearchQueryRuntime,
    query: &str,
    sources: &[SearchResultSource],
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    ensure_not_cancelled(cancel)?;
    let full_text_runtime = runtime.full_text_runtime()?;
    full_text_hits_to_matches(
        runtime,
        search_tantivy_index(full_text_runtime.as_deref(), query, sources, limit)?,
        cancel,
    )
}

fn full_text_hits_to_matches(
    runtime: &SearchQueryRuntime,
    hits: Vec<FullTextSearchHit>,
    cancel: &CancellationToken,
) -> Result<Vec<FileSearchMatch>, IndexError> {
    ensure_not_cancelled(cancel)?;
    let read_session = store::CatalogReadSession::open(runtime.index_dir(), runtime.root())?;
    let records_by_storage_key = read_session
        .read_records_by_storage_keys(hits.iter().map(|hit| hit.storage_key.as_str()))?;
    let mut matches = Vec::with_capacity(hits.len());
    for hit in hits {
        ensure_not_cancelled(cancel)?;
        let Some(record) = records_by_storage_key.get(&hit.storage_key) else {
            continue;
        };
        matches.push(record.to_search_match(hit.score, hit.source, hit.snippet, hit.media));
    }
    Ok(matches)
}

fn ensure_not_cancelled(cancel: &CancellationToken) -> Result<(), IndexError> {
    if cancel.is_cancelled() {
        Err(IndexError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn merge_search_matches(
    matches: Vec<FileSearchMatch>,
    limit: usize,
) -> Vec<FileSearchMatch> {
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
