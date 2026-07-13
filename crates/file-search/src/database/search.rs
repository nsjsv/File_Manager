use std::path::PathBuf;

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter};

use crate::error::SearchResult;
use crate::model::{
    MatchSource, SearchCursor, SearchFileKind, SearchHit, SearchQuery, SearchResultBatch,
    SearchScope,
};

use super::{path_to_storage, SearchDatabase};

const QUERY_VISIBLE_PREDICATE: &str = "f.tombstoned = 0 AND f.observation_state = 'observable'";
pub(super) const MAX_SEARCH_SNIPPET_BYTES: usize = 4_096;
pub(super) const MAX_SNIPPET_HITS_PER_BATCH: usize = 20;

impl SearchDatabase {
    pub fn search(&self, query: &SearchQuery) -> SearchResult<SearchResultBatch> {
        let limit = query.limit.clamp(1, 200);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let terms = search_match_expression(&query.terms);
        let has_terms = terms.is_some();
        let mut values = Vec::new();
        let mut sql = if has_terms {
            values.push(Value::Text(terms.clone().unwrap()));
            "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                    f.created_ms, f.mime_type, file_search_fts.rank AS score,
                    file_search_fts.rowid
             FROM file_search_fts
             JOIN files f ON f.rowid = file_search_fts.rowid
             WHERE file_search_fts MATCH ? AND "
                .to_owned()
                + QUERY_VISIBLE_PREDICATE
        } else {
            "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                    f.created_ms, f.mime_type, 0.0 AS score, f.rowid
             FROM files f
             WHERE "
                .to_owned()
                + QUERY_VISIBLE_PREDICATE
        };

        append_scope_filter(&mut sql, &mut values, &query.scope, query.recursive);
        append_filters(&mut sql, &mut values, query);
        if has_terms {
            // FTS5 可在原生 rank 顺序下提前满足 LIMIT；附加排序会强制扫描并排序全部命中。
            sql.push_str(" ORDER BY file_search_fts.rank LIMIT ? OFFSET ?");
        } else {
            sql.push_str(" ORDER BY f.modified_ms DESC, f.display_name ASC LIMIT ? OFFSET ?");
        }
        values.push(Value::Integer(limit as i64));
        values.push(Value::Integer(offset as i64));

        let mut statement = self.connection.prepare(&sql)?;
        let mut ranked_hits = statement
            .query_map(params_from_iter(values), |row| {
                let path_string: String = row.get(0)?;
                let display_name: String = row.get(1)?;
                let kind_string: String = row.get(2)?;
                let score: f64 = row.get(8)?;
                Ok((
                    SearchHit {
                        path: PathBuf::from(path_string),
                        display_name: display_name.clone(),
                        kind: SearchFileKind::from_storage_value(&kind_string),
                        size: row.get::<_, i64>(3)? as u64,
                        modified_ms: row.get(4)?,
                        accessed_ms: row.get(5)?,
                        created_ms: row.get(6)?,
                        rank: -score,
                        snippet: None,
                        match_source: match_source_for_hit(&query.terms, &display_name),
                    },
                    row.get::<_, i64>(9)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        if let Some(match_expression) = terms.as_deref() {
            let mut snippet_statement = self.connection.prepare_cached(
                "SELECT snippet(file_search_fts, 2, '<b>', '</b>', '...', 12)
                 FROM file_search_fts
                 WHERE rowid = ?1 AND file_search_fts MATCH ?2",
            )?;
            for (hit, rowid) in ranked_hits.iter_mut().take(MAX_SNIPPET_HITS_PER_BATCH) {
                let snippet: Option<String> = snippet_statement
                    .query_row(params![*rowid, match_expression], |row| row.get(0))?;
                hit.snippet = snippet
                    .filter(|snippet| !snippet.is_empty())
                    .map(|snippet| truncate_utf8(snippet, MAX_SEARCH_SNIPPET_BYTES));
            }
        }

        let hits = ranked_hits
            .into_iter()
            .map(|(hit, _)| hit)
            .collect::<Vec<_>>();

        let finished = hits.len() < limit;
        let next_cursor = (!finished).then_some(SearchCursor {
            offset: offset + hits.len(),
        });
        Ok(SearchResultBatch {
            query_id: query.query_id,
            hits,
            next_cursor,
            finished,
        })
    }

    pub fn indexed_file_count(&self) -> SearchResult<u64> {
        let sql = format!("SELECT COUNT(*) FROM files f WHERE {QUERY_VISIBLE_PREDICATE}");
        let count: i64 = self.connection.query_row(&sql, [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }
}

fn truncate_utf8(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text
}

fn append_scope_filter(
    sql: &mut String,
    values: &mut Vec<Value>,
    scope: &SearchScope,
    recursive: bool,
) {
    let SearchScope::Directory(directory) = scope else {
        return;
    };
    if recursive {
        let directory = path_to_storage(directory);
        let prefix = format!("{directory}/");
        sql.push_str(" AND (f.path = ? OR substr(f.path, 1, length(?)) = ?)");
        values.push(Value::Text(directory));
        values.push(Value::Text(prefix.clone()));
        values.push(Value::Text(prefix));
    } else {
        sql.push_str(" AND f.parent_path = ?");
        values.push(Value::Text(path_to_storage(directory)));
    }
}

fn append_filters(sql: &mut String, values: &mut Vec<Value>, query: &SearchQuery) {
    if let Some(kind) = query.filters.kind {
        sql.push_str(" AND f.kind = ?");
        values.push(Value::Text(kind.as_storage_value().to_owned()));
    }
    if let Some(mime_type) = &query.filters.mime_type {
        sql.push_str(" AND f.mime_type = ?");
        values.push(Value::Text(mime_type.clone()));
    }
    append_time_filter(sql, values, "f.modified_ms", query.filters.modified);
    append_time_filter(sql, values, "f.accessed_ms", query.filters.accessed);
    append_time_filter(sql, values, "f.created_ms", query.filters.created);
}

fn append_time_filter(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    range: Option<crate::model::TimeRange>,
) {
    if let Some(range) = range {
        sql.push_str(" AND ");
        sql.push_str(column);
        sql.push_str(" >= ? AND ");
        sql.push_str(column);
        sql.push_str(" <= ?");
        values.push(Value::Integer(range.start_ms));
        values.push(Value::Integer(range.end_ms));
    }
}

fn search_match_expression(terms: &str) -> Option<String> {
    let tokens = terms
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!tokens.is_empty()).then(|| tokens.join(" AND "))
}

fn match_source_for_hit(terms: &str, display_name: &str) -> MatchSource {
    let display_name = display_name.to_ascii_lowercase();
    if terms
        .split_whitespace()
        .any(|term| display_name.contains(&term.to_ascii_lowercase()))
    {
        MatchSource::Name
    } else if terms.trim().is_empty() {
        MatchSource::Metadata
    } else {
        MatchSource::Content
    }
}
