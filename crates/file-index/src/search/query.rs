use std::cmp::Ordering;
use std::collections::HashSet;

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher};

use super::catalog::{normalize_search_text, SearchCatalog, SearchCatalogRecord};
use super::types::FileSearchMatch;

const EXACT_NAME_SCORE: u32 = 1_000_000;
const NAME_PREFIX_SCORE: u32 = 900_000;
const SEGMENT_PREFIX_SCORE: u32 = 820_000;
const NAME_SUBSTRING_SCORE: u32 = 760_000;
const PATH_SUBSTRING_SCORE: u32 = 680_000;
const FUZZY_NAME_BONUS: u32 = 20_000;
const FUZZY_PATH_BONUS: u32 = 10_000;
const MAX_RANKED_CANDIDATES: usize = 50_000;

pub(crate) struct SearchMatchCollector {
    normalized_query: String,
    pattern: Pattern,
    matcher: Matcher,
    limit: usize,
    spill_limit: usize,
    matches: Vec<FileSearchMatch>,
}

impl SearchMatchCollector {
    pub(crate) fn new(query: &str, limit: usize) -> Self {
        let limit = limit.max(1);
        Self {
            normalized_query: normalize_search_text(query.trim()),
            pattern: Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart),
            matcher: Matcher::new(Config::DEFAULT),
            limit,
            spill_limit: limit.saturating_mul(2).max(128),
            matches: Vec::new(),
        }
    }

    pub(crate) fn push_record(&mut self, record: &SearchCatalogRecord) {
        if self.normalized_query.is_empty() {
            return;
        }
        let Some(rank_score) = rank_record(
            record,
            &self.normalized_query,
            &self.pattern,
            &mut self.matcher,
        ) else {
            return;
        };
        self.matches.push(record.to_match(rank_score));
        if self.matches.len() > self.spill_limit {
            sort_limited_search_matches(&mut self.matches, self.limit);
        }
    }

    pub(crate) fn finish(mut self) -> Vec<FileSearchMatch> {
        sort_limited_search_matches(&mut self.matches, self.limit);
        self.matches
    }
}

pub(crate) fn search_catalog(
    catalog: &SearchCatalog,
    query: &str,
    limit: usize,
) -> Vec<FileSearchMatch> {
    let normalized_query = normalize_search_text(query.trim());
    if normalized_query.is_empty() {
        return Vec::new();
    }

    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let candidate_indices = collect_candidate_indices(catalog, &normalized_query, limit);
    let mut matches = candidate_indices
        .into_iter()
        .filter_map(|index| {
            let record = catalog.records().get(index)?;
            let rank_score = rank_record(record, &normalized_query, &pattern, &mut matcher)?;
            Some(record.to_match(rank_score))
        })
        .collect::<Vec<_>>();

    sort_limited_search_matches(&mut matches, limit);
    matches
}

fn collect_candidate_indices(
    catalog: &SearchCatalog,
    normalized_query: &str,
    limit: usize,
) -> Vec<usize> {
    let fallback_target = limit.saturating_mul(200).clamp(512, MAX_RANKED_CANDIDATES);
    let query_len = normalized_query.chars().count();
    let mut indices = if query_len >= 3 {
        catalog.trigram_candidates(normalized_query)
    } else {
        Vec::new()
    };

    if query_len < 3 || indices.len() < fallback_target {
        let mut seen = indices.iter().copied().collect::<HashSet<_>>();
        for (index, record) in catalog.records().iter().enumerate() {
            let matches = if query_len < 3 {
                structural_rank(record, normalized_query).is_some()
                    || ordered_match(&record.normalized_name, normalized_query)
                    || ordered_match(&record.normalized_path, normalized_query)
            } else {
                ordered_match(&record.normalized_name, normalized_query)
                    || ordered_match(&record.normalized_path, normalized_query)
            };
            if matches {
                push_candidate_index(index, &mut indices, &mut seen);
            }
            if indices.len() >= MAX_RANKED_CANDIDATES {
                break;
            }
        }
    }

    if indices.len() > MAX_RANKED_CANDIDATES {
        indices.truncate(MAX_RANKED_CANDIDATES);
    }
    indices
}

fn push_candidate_index(index: usize, indices: &mut Vec<usize>, seen: &mut HashSet<usize>) {
    if seen.insert(index) {
        indices.push(index);
    }
}

fn rank_record(
    record: &SearchCatalogRecord,
    normalized_query: &str,
    pattern: &Pattern,
    matcher: &mut Matcher,
) -> Option<u32> {
    let structural_score = structural_rank(record, normalized_query);
    let name_score = pattern
        .score(record.name_utf32.slice(..), matcher)
        .map(|score| score.saturating_add(FUZZY_NAME_BONUS));
    let path_score = pattern
        .score(record.path_utf32.slice(..), matcher)
        .map(|score| score.saturating_add(FUZZY_PATH_BONUS));
    let fuzzy_score = name_score.max(path_score);
    structural_score.max(fuzzy_score)
}

fn structural_rank(record: &SearchCatalogRecord, normalized_query: &str) -> Option<u32> {
    if record.normalized_name == normalized_query {
        Some(EXACT_NAME_SCORE.saturating_add(short_path_bonus(record)))
    } else if record.normalized_name.starts_with(normalized_query) {
        Some(NAME_PREFIX_SCORE.saturating_add(short_path_bonus(record)))
    } else if record.segment_starts_with(normalized_query) {
        Some(SEGMENT_PREFIX_SCORE.saturating_add(short_path_bonus(record)))
    } else if record.normalized_name.contains(normalized_query) {
        Some(NAME_SUBSTRING_SCORE.saturating_add(short_path_bonus(record)))
    } else if record.normalized_path.contains(normalized_query) {
        Some(PATH_SUBSTRING_SCORE.saturating_add(short_path_bonus(record)))
    } else {
        None
    }
}

fn short_path_bonus(record: &SearchCatalogRecord) -> u32 {
    1_000u32.saturating_sub(record.path_text.chars().count().min(1_000) as u32)
}

fn ordered_match(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let mut chars = text.chars();
    query
        .chars()
        .all(|query_char| chars.by_ref().any(|text_char| text_char == query_char))
}

fn sort_limited_search_matches(matches: &mut Vec<FileSearchMatch>, limit: usize) {
    if matches.len() > limit {
        matches.select_nth_unstable_by(limit, compare_search_matches);
        matches.truncate(limit);
    }
    matches.sort_unstable_by(compare_search_matches);
}

fn compare_search_matches(left: &FileSearchMatch, right: &FileSearchMatch) -> Ordering {
    right
        .rank_score
        .cmp(&left.rank_score)
        .then_with(|| left.path.cmp(&right.path))
}
