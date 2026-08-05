use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::error::SearchResult;
use crate::model::{
    MatchSource, SearchCursor, SearchEntryTypeRule, SearchFileKind, SearchHit, SearchQuery,
    SearchResultBatch, SearchScope, SearchTextScope,
};

use super::{
    path_from_storage_bytes, path_to_storage, recursive_storage_range, RecursiveStorageRange,
    SearchDatabase,
};

const QUERY_VISIBLE_PREDICATE: &str = "f.tombstoned = 0 AND f.observation_state = 'observable'";
const SEARCH_SNIPPET_CHARACTER_LIMIT: usize = 240;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FullTextQueryPlan {
    FilterWithMetadata,
    UnfilteredRank,
}

impl SearchDatabase {
    pub fn search(&self, query: &SearchQuery) -> SearchResult<SearchResultBatch> {
        let limit = query.limit.clamp(1, 200);
        let fetch_limit = limit.saturating_add(1);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let tokens = search_tokens(&query.terms);
        let transaction = self.connection.unchecked_transaction()?;
        let full_text_plan = full_text_query_plan(&transaction, query)?;
        let (sql, values) = search_sql(query, &tokens, fetch_limit, offset, full_text_plan);

        let mut hits = {
            let mut statement = transaction.prepare(&sql)?;
            let rows = statement
                .query_map(params_from_iter(values), |row| {
                    let path_bytes: Vec<u8> = row.get(0)?;
                    let display_name: String = row.get(1)?;
                    let kind = SearchFileKind::from_storage_value(row.get_ref(2)?.as_str()?);
                    let score: f64 = row.get(7)?;
                    let raw_snippet: Option<String> = row.get(8)?;
                    let match_source =
                        match_source_for_hit(query.text_scope, &tokens, &display_name);
                    let snippet = if match_source == MatchSource::Content {
                        raw_snippet.and_then(|snippet| normalize_content_snippet(&snippet))
                    } else {
                        None
                    };
                    Ok(SearchHit {
                        path: path_from_storage_bytes(path_bytes),
                        display_name,
                        kind,
                        size: row.get::<_, i64>(3)? as u64,
                        modified_ms: row.get(4)?,
                        accessed_ms: row.get(5)?,
                        created_ms: row.get(6)?,
                        rank: -score,
                        snippet,
                        match_source,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        transaction.commit()?;

        let has_more = hits.len() > limit;
        hits.truncate(limit);
        let next_cursor = has_more.then_some(SearchCursor {
            offset: offset + hits.len(),
        });
        Ok(SearchResultBatch {
            query_id: query.query_id,
            hits,
            next_cursor,
            finished: !has_more,
        })
    }

    #[cfg(test)]
    pub(super) fn search_plan(&self, query: &SearchQuery) -> SearchResult<Vec<String>> {
        let limit = query.limit.clamp(1, 200).saturating_add(1);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let tokens = search_tokens(&query.terms);
        let full_text_plan = full_text_query_plan(&self.connection, query)?;
        let (sql, values) = search_sql(query, &tokens, limit, offset, full_text_plan);
        let mut statement = self
            .connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let plan = statement
            .query_map(params_from_iter(values), |row| row.get(3))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into);
        plan
    }

    #[cfg(test)]
    pub(super) fn projected_content_snippets(
        &self,
        query: &SearchQuery,
    ) -> SearchResult<Vec<(std::path::PathBuf, Option<String>)>> {
        let limit = query.limit.clamp(1, 200).saturating_add(1);
        let offset = query.cursor.map_or(0, |cursor| cursor.offset);
        let tokens = search_tokens(&query.terms);
        let full_text_plan = full_text_query_plan(&self.connection, query)?;
        let (sql, values) = search_sql(query, &tokens, limit, offset, full_text_plan);
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                let path: Vec<u8> = row.get(0)?;
                Ok((path_from_storage_bytes(path), row.get(8)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
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
        && query.filters.entry_type_rules.is_empty()
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
        FullTextQueryPlan::UnfilteredRank
    })
}

fn search_sql(
    query: &SearchQuery,
    tokens: &[&str],
    limit: usize,
    offset: usize,
    full_text_plan: FullTextQueryPlan,
) -> (String, Vec<Value>) {
    let match_expression = search_match_expression(tokens, query.text_scope);
    let recursive_range = recursive_path_range(query);
    let range_drives_query = match_expression.is_none() && recursive_range.is_some();
    let mut values = Vec::new();
    let mut sql = if let Some(match_expression) = match_expression.as_ref() {
        let snippet_projection = content_snippet_projection(tokens, query.text_scope, &mut values);
        values.push(Value::Text(match_expression.clone()));
        let mut full_text_sql = format!(
            "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                f.created_ms, file_search_fts.rank AS score,
                {snippet_projection}
         FROM file_search_fts
         JOIN files f ON f.rowid = file_search_fts.rowid
         WHERE file_search_fts MATCH ?"
        );
        if full_text_plan == FullTextQueryPlan::FilterWithMetadata {
            full_text_sql.push_str(" AND ");
            full_text_sql.push_str(QUERY_VISIBLE_PREDICATE);
        }
        full_text_sql
    } else if let Some(range) = recursive_range.as_ref() {
        let mut scoped_sql = "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms,
                    f.accessed_ms, f.created_ms, 0.0 AS score, NULL AS content_snippet
             FROM files f
             WHERE f.rowid IN (SELECT rowid FROM files WHERE "
            .to_owned();
        append_recursive_path_range(&mut scoped_sql, &mut values, "path", range);
        scoped_sql.push_str(") AND ");
        scoped_sql + QUERY_VISIBLE_PREDICATE
    } else {
        "SELECT f.path, f.display_name, f.kind, f.size, f.modified_ms, f.accessed_ms,
                f.created_ms, 0.0 AS score, NULL AS content_snippet
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
        // FTS5 可在原生 rank 顺序下提前满足 LIMIT，避免评分并排序全部命中。
        sql.push_str(" ORDER BY file_search_fts.rank LIMIT ? OFFSET ?");
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
        values.push(Value::Blob(path_to_storage(directory)));
    }
}

fn recursive_path_range(query: &SearchQuery) -> Option<RecursiveStorageRange> {
    if !query.recursive {
        return None;
    }
    let SearchScope::Directory(directory) = &query.scope else {
        return None;
    };
    Some(recursive_storage_range(directory))
}

fn append_recursive_path_range(
    sql: &mut String,
    values: &mut Vec<Value>,
    column: &str,
    range: &RecursiveStorageRange,
) {
    sql.push('(');
    sql.push_str(column);
    sql.push_str(" = ? OR (");
    values.push(Value::Blob(range.exact_path.clone()));
    sql.push_str(column);
    sql.push_str(" >= ? AND ");
    sql.push_str(column);
    sql.push_str(" < ?))");
    values.push(Value::Blob(range.descendant_lower.clone()));
    values.push(Value::Blob(range.descendant_upper.clone()));
}

fn append_filters(sql: &mut String, values: &mut Vec<Value>, query: &SearchQuery) {
    append_entry_type_rules(sql, values, &query.filters.entry_type_rules);
    append_time_filter(sql, values, "f.modified_ms", query.filters.modified);
    append_time_filter(sql, values, "f.accessed_ms", query.filters.accessed);
    append_time_filter(sql, values, "f.created_ms", query.filters.created);
}

fn append_entry_type_rules(
    sql: &mut String,
    values: &mut Vec<Value>,
    rules: &[SearchEntryTypeRule],
) {
    if rules.is_empty() {
        return;
    }
    sql.push_str(" AND (");
    for (index, rule) in rules.iter().enumerate() {
        if index > 0 {
            sql.push_str(" OR ");
        }
        match rule {
            SearchEntryTypeRule::Kind(kind) => {
                sql.push_str("f.kind = ?");
                values.push(Value::Text(kind.as_storage_value().to_owned()));
            }
            SearchEntryTypeRule::Mime(crate::model::MimePattern::Exact(mime_type)) => {
                sql.push_str("f.mime_type = ?");
                values.push(Value::Text(mime_type.clone()));
            }
            SearchEntryTypeRule::Mime(crate::model::MimePattern::Prefix(prefix)) => {
                sql.push_str("f.mime_type LIKE ? ESCAPE '\\'");
                values.push(Value::Text(escaped_like_prefix(prefix)));
            }
        }
    }
    sql.push(')');
}

fn escaped_like_prefix(prefix: &str) -> String {
    let mut pattern = String::with_capacity(prefix.len() + 1);
    for character in prefix.chars() {
        if matches!(character, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
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

fn search_tokens(terms: &str) -> Vec<&str> {
    terms.split_whitespace().collect()
}

fn search_match_expression(tokens: &[&str], text_scope: SearchTextScope) -> Option<String> {
    let tokens = tokens
        .iter()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }
    let expression = tokens.join(" AND ");
    Some(match text_scope {
        SearchTextScope::NameAndContent => expression,
        SearchTextScope::NameOnly => format!("name : ({expression})"),
    })
}

fn content_snippet_projection(
    tokens: &[&str],
    text_scope: SearchTextScope,
    values: &mut Vec<Value>,
) -> String {
    if text_scope == SearchTextScope::NameOnly {
        return "NULL AS content_snippet".to_owned();
    }
    debug_assert!(!tokens.is_empty());
    let mut projection = "CASE WHEN ".to_owned();
    for (index, token) in tokens.iter().enumerate() {
        if index > 0 {
            projection.push_str(" AND ");
        }
        projection.push_str("instr(lower(f.display_name), lower(?)) > 0");
        values.push(Value::Text((*token).to_owned()));
    }
    projection.push_str(
        " THEN NULL ELSE snippet(file_search_fts, 2, '', '', '...', 16) END AS content_snippet",
    );
    projection
}

fn match_source_for_hit(
    text_scope: SearchTextScope,
    terms: &[&str],
    display_name: &str,
) -> MatchSource {
    if terms.is_empty() {
        return MatchSource::Metadata;
    }
    if text_scope == SearchTextScope::NameOnly {
        return MatchSource::Name;
    }
    let display_name = display_name.as_bytes();
    if terms.iter().all(|term| {
        let term = term.as_bytes();
        display_name
            .windows(term.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(term))
    }) {
        MatchSource::Name
    } else {
        MatchSource::Content
    }
}

fn normalize_content_snippet(snippet: &str) -> Option<String> {
    let mut normalized = String::with_capacity(snippet.len());
    let mut pending_space = false;
    for character in snippet.chars() {
        if character.is_whitespace() || character.is_control() {
            pending_space = !normalized.is_empty();
            continue;
        }
        if pending_space {
            normalized.push(' ');
            pending_space = false;
        }
        normalized.push(character);
    }
    if normalized.is_empty() {
        return None;
    }
    if normalized.chars().count() > SEARCH_SNIPPET_CHARACTER_LIMIT {
        normalized = normalized
            .chars()
            .take(SEARCH_SNIPPET_CHARACTER_LIMIT.saturating_sub(3))
            .collect::<String>();
        normalized.push_str("...");
    }
    Some(normalized)
}
