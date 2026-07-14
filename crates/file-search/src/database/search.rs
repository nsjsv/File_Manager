use std::path::PathBuf;

use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::error::SearchResult;
use crate::model::{
    MatchSource, SearchCursor, SearchFileKind, SearchHit, SearchQuery, SearchResultBatch,
    SearchScope,
};

use super::{path_to_storage, SearchDatabase};

const QUERY_VISIBLE_PREDICATE: &str = "f.tombstoned = 0 AND f.observation_state = 'observable'";

struct RecursivePathRange {
    exact_path: Option<String>,
    descendant_lower: String,
    descendant_upper: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FullTextQueryPlan {
    FilterWithMetadata,
    RankBeforeMetadata,
}

impl SearchDatabase {
    pub fn search(&self, query: &SearchQuery) -> SearchResult<SearchResultBatch> {
        let limit = query.limit.clamp(1, 200);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let transaction = self.connection.unchecked_transaction()?;
        let full_text_plan = full_text_query_plan(&transaction, query)?;
        let (sql, values) = search_sql(query, limit, offset, full_text_plan);

        let hits = {
            let mut statement = transaction.prepare(&sql)?;
            let rows = statement
                .query_map(params_from_iter(values), |row| {
                    let path_string: String = row.get(0)?;
                    let display_name: String = row.get(1)?;
                    let kind = SearchFileKind::from_storage_value(row.get_ref(2)?.as_str()?);
                    let score: f64 = row.get(7)?;
                    let match_source = match_source_for_hit(&query.terms, &display_name);
                    Ok(SearchHit {
                        path: PathBuf::from(path_string),
                        display_name,
                        kind,
                        size: row.get::<_, i64>(3)? as u64,
                        modified_ms: row.get(4)?,
                        accessed_ms: row.get(5)?,
                        created_ms: row.get(6)?,
                        rank: -score,
                        snippet: None,
                        match_source,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        transaction.commit()?;

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

    #[cfg(test)]
    pub(super) fn search_plan(&self, query: &SearchQuery) -> SearchResult<Vec<String>> {
        let limit = query.limit.clamp(1, 200);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let full_text_plan = full_text_query_plan(&self.connection, query)?;
        let (sql, values) = search_sql(query, limit, offset, full_text_plan);
        let mut statement = self
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let plan = statement
            .query_map(params_from_iter(values), |row| row.get(3))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into);
        plan
    }

    pub fn indexed_file_count(&self) -> SearchResult<u64> {
        let sql = format!("SELECT COUNT(*) FROM files f WHERE {QUERY_VISIBLE_PREDICATE}");
        let count: i64 = self.connection.query_row(&sql, [], |row| row.get(0))?;
        Ok(count.max(0) as u64)
    }
}

fn full_text_query_plan(
    connection: &rusqlite::Connection,
    query: &SearchQuery,
) -> SearchResult<FullTextQueryPlan> {
    let unrestricted_full_text_query = query.terms.split_whitespace().next().is_some()
        && matches!(query.scope, SearchScope::Global)
        && query.filters.kind.is_none()
        && query.filters.mime_type.is_none()
        && query.filters.modified.is_none()
        && query.filters.accessed.is_none()
        && query.filters.created.is_none();
    if !unrestricted_full_text_query {
        return Ok(FullTextQueryPlan::FilterWithMetadata);
    }
    let hidden_rows_exist = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM files
            WHERE tombstoned <> 0 OR observation_state <> 'observable'
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    Ok(if hidden_rows_exist {
        FullTextQueryPlan::FilterWithMetadata
    } else {
        FullTextQueryPlan::RankBeforeMetadata
    })
}

fn search_sql(
    query: &SearchQuery,
    limit: usize,
    offset: usize,
    full_text_plan: FullTextQueryPlan,
) -> (String, Vec<Value>) {
    let match_expression = search_match_expression(&query.terms);
    if let (Some(match_expression), FullTextQueryPlan::RankBeforeMetadata) =
        (match_expression.as_ref(), full_text_plan)
    {
        return (
            "WITH ranked AS MATERIALIZED (
                SELECT file_search_fts.rowid AS rowid, bm25(file_search_fts) AS score
                FROM file_search_fts
                WHERE file_search_fts MATCH ?
                ORDER BY score LIMIT ? OFFSET ?
             )
             SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                    f.created_ms, ranked.score
             FROM ranked
             JOIN files f ON f.rowid = ranked.rowid
             ORDER BY ranked.score"
                .to_owned(),
            vec![
                Value::Text(match_expression.clone()),
                Value::Integer(limit as i64),
                Value::Integer(offset as i64),
            ],
        );
    }
    let recursive_range = recursive_path_range(query);
    let range_drives_query = match_expression.is_none() && recursive_range.is_some();
    let mut values = Vec::new();
    let mut sql = if let Some(match_expression) = match_expression.as_ref() {
        values.push(Value::Text(match_expression.clone()));
        "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                f.created_ms, bm25(file_search_fts) AS score
         FROM file_search_fts
         JOIN files f ON f.rowid = file_search_fts.rowid
         WHERE file_search_fts MATCH ? AND "
            .to_owned()
            + QUERY_VISIBLE_PREDICATE
    } else if let Some(range) = recursive_range.as_ref() {
        let mut scoped_sql = "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms,
                    f.accessed_ms, f.created_ms, 0.0 AS score
             FROM files f
             WHERE f.rowid IN (SELECT rowid FROM files WHERE "
            .to_owned();
        append_recursive_path_range(&mut scoped_sql, &mut values, "path", range);
        scoped_sql.push_str(") AND ");
        scoped_sql + QUERY_VISIBLE_PREDICATE
    } else {
        "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                f.created_ms, 0.0 AS score
         FROM files f
         WHERE "
            .to_owned()
            + QUERY_VISIBLE_PREDICATE
    };

    if !range_drives_query {
        append_scope_filter(&mut sql, &mut values, query);
    }
    append_filters(&mut sql, &mut values, query);
    if match_expression.is_some() {
        // bundled SQLite 3.46 可复用投影分数，且与默认 rank 的分页顺序等价。
        sql.push_str(" ORDER BY score LIMIT ? OFFSET ?");
    } else {
        sql.push_str(" ORDER BY f.modified_ms DESC, f.display_name ASC LIMIT ? OFFSET ?");
    }
    values.push(Value::Integer(limit as i64));
    values.push(Value::Integer(offset as i64));
    (sql, values)
}

fn append_scope_filter(sql: &mut String, values: &mut Vec<Value>, query: &SearchQuery) {
    let SearchScope::Directory(directory) = &query.scope else {
        return;
    };
    if query.recursive {
        let range = recursive_path_range(query).expect("recursive directory range");
        sql.push_str(" AND ");
        append_recursive_path_range(sql, values, "f.path", &range);
    } else {
        sql.push_str(" AND f.parent_path = ?");
        values.push(Value::Text(path_to_storage(directory)));
    }
}

fn recursive_path_range(query: &SearchQuery) -> Option<RecursivePathRange> {
    if !query.recursive {
        return None;
    }
    let SearchScope::Directory(directory) = &query.scope else {
        return None;
    };
    let directory = path_to_storage(directory);
    if directory == "/" {
        return Some(RecursivePathRange {
            exact_path: None,
            descendant_lower: "/".to_owned(),
            descendant_upper: "0".to_owned(),
        });
    }
    Some(RecursivePathRange {
        exact_path: Some(directory.clone()),
        descendant_lower: format!("{directory}/"),
        descendant_upper: format!("{directory}0"),
    })
}

fn append_recursive_path_range(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    range: &RecursivePathRange,
) {
    if let Some(exact_path) = &range.exact_path {
        sql.push('(');
        sql.push_str(column);
        sql.push_str(" = ? OR (");
        values.push(Value::Text(exact_path.clone()));
    } else {
        sql.push('(');
    }
    sql.push_str(column);
    sql.push_str(" >= ? AND ");
    sql.push_str(column);
    sql.push_str(" < ?)");
    values.push(Value::Text(range.descendant_lower.clone()));
    values.push(Value::Text(range.descendant_upper.clone()));
    if range.exact_path.is_some() {
        sql.push(')');
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
    let display_name = display_name.as_bytes();
    if terms.split_whitespace().any(|term| {
        let term = term.as_bytes();
        display_name
            .windows(term.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(term))
    }) {
        MatchSource::Name
    } else if terms.trim().is_empty() {
        MatchSource::Metadata
    } else {
        MatchSource::Content
    }
}
