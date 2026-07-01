use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::{
    BatchRenamePreview, BatchRenamePreviewRow, BatchRenamePreviewStatus, BatchRenameSortMode,
    BatchRenameSortRule, BatchRenameSource, BatchRenameState,
};
use crate::model::batch_rename::transforms::PreparedBatchRenameRules;

pub(super) fn build_batch_rename_preview(state: &BatchRenameState) -> BatchRenamePreview {
    let sorted_items = sorted_batch_rename_items(&state.items, &state.sort);
    let prepared_rules = PreparedBatchRenameRules::new(state);
    let mut rows = Vec::with_capacity(sorted_items.len());

    for (index, item) in sorted_items.into_iter().enumerate() {
        let target_name_result = prepared_rules.rename_item_name(item, index, state);
        let has_rule_error = target_name_result.is_err();
        let rule_target_name = target_name_result.unwrap_or_else(|_| item.name.clone());
        let target_name = state
            .manual_target_name_override(&item.path)
            .map(ToOwned::to_owned)
            .unwrap_or(rule_target_name);
        let target = item
            .path
            .parent()
            .map(|parent| parent.join(&target_name))
            .unwrap_or_else(|| PathBuf::from(&target_name));
        rows.push(BatchRenamePreviewRow {
            source: item.path.clone(),
            original_name: item.name.clone(),
            target,
            target_name,
            status: if has_rule_error {
                BatchRenamePreviewStatus::RuleError
            } else {
                BatchRenamePreviewStatus::Ready
            },
        });
    }

    mark_batch_rename_preview_statuses(&mut rows, &state.existing_paths);
    BatchRenamePreview { rows }
}

fn mark_batch_rename_preview_statuses(
    rows: &mut [BatchRenamePreviewRow],
    existing_paths: &HashSet<PathBuf>,
) {
    let mut target_counts = HashMap::<PathBuf, usize>::new();
    let source_paths = rows
        .iter()
        .map(|row| row.source.clone())
        .collect::<HashSet<_>>();
    for row in rows.iter() {
        *target_counts.entry(row.target.clone()).or_default() += 1;
    }

    for row in rows {
        row.status = if row.status == BatchRenamePreviewStatus::RuleError {
            BatchRenamePreviewStatus::RuleError
        } else if row.target_name.is_empty() {
            BatchRenamePreviewStatus::EmptyName
        } else if target_counts.get(&row.target).copied().unwrap_or(0) > 1 {
            BatchRenamePreviewStatus::DuplicateTarget
        } else if existing_paths.contains(&row.target) && !source_paths.contains(&row.target) {
            BatchRenamePreviewStatus::ExistingTarget
        } else if row.source == row.target {
            BatchRenamePreviewStatus::Unchanged
        } else {
            BatchRenamePreviewStatus::Ready
        };
    }
}

fn sorted_batch_rename_items<'a>(
    items: &'a [BatchRenameSource],
    sort: &BatchRenameSortRule,
) -> Vec<&'a BatchRenameSource> {
    let mut sorted = items.iter().collect::<Vec<_>>();
    match sort.mode {
        BatchRenameSortMode::SelectionOrder => {}
        BatchRenameSortMode::NaturalAscending => {
            sorted.sort_by(|left, right| natural_name_cmp(&left.name, &right.name));
        }
        BatchRenameSortMode::NameAscending => {
            sorted.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        }
        BatchRenameSortMode::NameDescending => {
            sorted.sort_by(|left, right| right.name.to_lowercase().cmp(&left.name.to_lowercase()));
        }
        BatchRenameSortMode::ModifiedAscending => {
            sorted.sort_by(|left, right| modified_sort_key(left).cmp(&modified_sort_key(right)));
        }
        BatchRenameSortMode::ModifiedDescending => {
            sorted.sort_by(|left, right| modified_sort_key(right).cmp(&modified_sort_key(left)));
        }
        BatchRenameSortMode::Random => {
            sorted.sort_by_key(|item| deterministic_sort_key(item));
        }
        BatchRenameSortMode::ExtensionAscending => {
            sorted.sort_by(|left, right| {
                file_extension_for_sort(&left.name).cmp(&file_extension_for_sort(&right.name))
            });
        }
        BatchRenameSortMode::ExtensionDescending => {
            sorted.sort_by(|left, right| {
                file_extension_for_sort(&right.name).cmp(&file_extension_for_sort(&left.name))
            });
        }
        BatchRenameSortMode::Reverse => sorted.reverse(),
    }
    sorted
}

fn file_extension_for_sort(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn modified_sort_key(item: &BatchRenameSource) -> (bool, Option<std::time::SystemTime>, String) {
    (
        item.modified.is_none(),
        item.modified,
        item.name.to_lowercase(),
    )
}

fn deterministic_sort_key(item: &BatchRenameSource) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(item.path.as_os_str().to_string_lossy().as_bytes());
    if let Some(modified) = item.modified {
        if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
            hasher.update(&duration.as_secs().to_le_bytes());
            hasher.update(&duration.subsec_nanos().to_le_bytes());
        }
    }
    *hasher.finalize().as_bytes()
}

fn natural_name_cmp(left: &str, right: &str) -> Ordering {
    let mut left_chars = left.chars().peekable();
    let mut right_chars = right.chars().peekable();

    loop {
        match (left_chars.peek().copied(), right_chars.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_char), Some(right_char)) => {
                if left_char.is_ascii_digit() && right_char.is_ascii_digit() {
                    let left_number = consume_ascii_digits(&mut left_chars);
                    let right_number = consume_ascii_digits(&mut right_chars);
                    match compare_ascii_digit_runs(&left_number, &right_number) {
                        Ordering::Equal => continue,
                        ordering => return ordering,
                    }
                }

                let left_lower = left_char.to_ascii_lowercase();
                let right_lower = right_char.to_ascii_lowercase();
                match left_lower.cmp(&right_lower) {
                    Ordering::Equal => {
                        left_chars.next();
                        right_chars.next();
                    }
                    ordering => return ordering,
                }
            }
        }
    }
}

fn consume_ascii_digits(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut digits = String::new();
    while let Some(character) = chars.peek().copied() {
        if !character.is_ascii_digit() {
            break;
        }
        digits.push(character);
        chars.next();
    }
    digits
}

fn compare_ascii_digit_runs(left: &str, right: &str) -> Ordering {
    let left_trimmed = left.trim_start_matches('0');
    let right_trimmed = right.trim_start_matches('0');
    let left_normalized = if left_trimmed.is_empty() {
        "0"
    } else {
        left_trimmed
    };
    let right_normalized = if right_trimmed.is_empty() {
        "0"
    } else {
        right_trimmed
    };
    match left_normalized.len().cmp(&right_normalized.len()) {
        Ordering::Equal => match left_normalized.cmp(right_normalized) {
            Ordering::Equal => left.len().cmp(&right.len()),
            ordering => ordering,
        },
        ordering => ordering,
    }
}
