use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use file_core::BatchRenameItem;

#[derive(Debug, Clone)]
pub(crate) enum BatchRenameMessage {
    OpenSelected,
    SequencePrefixChanged(String),
    SequenceStartChanged(String),
    SequencePaddingChanged(String),
    SequenceIncludeOriginalToggled(bool),
    SequencePreserveExtensionToggled(bool),
    ReplaceFindChanged(String),
    ReplaceWithChanged(String),
    InsertTextChanged(String),
    InsertPositionChanged(String),
    SliceStartChanged(String),
    SliceLengthChanged(String),
    CaseSelected(BatchRenameCaseRule),
    Apply,
    Cancel,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchRenameState {
    pub(crate) items: Vec<BatchRenameSource>,
    pub(crate) sequence: BatchRenameSequenceRule,
    pub(crate) replace: BatchRenameReplaceRule,
    pub(crate) insert: BatchRenameInsertRule,
    pub(crate) slice: BatchRenameSliceRule,
    pub(crate) case: BatchRenameCaseRule,
    existing_paths: HashSet<PathBuf>,
    pub(crate) preview: BatchRenamePreview,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchRenameSource {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameSequenceRule {
    pub(crate) prefix: String,
    pub(crate) start_input: String,
    pub(crate) padding_input: String,
    pub(crate) include_original_stem: bool,
    pub(crate) preserve_extension: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameReplaceRule {
    pub(crate) find: String,
    pub(crate) replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameInsertRule {
    pub(crate) text: String,
    pub(crate) position_input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameSliceRule {
    pub(crate) start_input: String,
    pub(crate) length_input: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameCaseRule {
    Unchanged,
    Lowercase,
    Uppercase,
    TitleCase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenamePreview {
    pub(crate) rows: Vec<BatchRenamePreviewRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenamePreviewRow {
    pub(crate) source: PathBuf,
    pub(crate) original_name: String,
    pub(crate) target: PathBuf,
    pub(crate) target_name: String,
    pub(crate) status: BatchRenamePreviewStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenamePreviewStatus {
    Ready,
    Unchanged,
    EmptyName,
    DuplicateTarget,
    ExistingTarget,
}

impl BatchRenameState {
    pub(crate) fn new_with_existing_paths(
        paths: Vec<PathBuf>,
        existing_paths: HashSet<PathBuf>,
    ) -> Option<Self> {
        let items = paths
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some(BatchRenameSource { path, name })
            })
            .collect::<Vec<_>>();
        if items.len() < 2 {
            return None;
        }

        let mut state = Self {
            items,
            sequence: BatchRenameSequenceRule::default(),
            replace: BatchRenameReplaceRule::default(),
            insert: BatchRenameInsertRule::default(),
            slice: BatchRenameSliceRule::default(),
            case: BatchRenameCaseRule::Unchanged,
            existing_paths,
            preview: BatchRenamePreview { rows: Vec::new() },
        };
        state.rebuild_preview();
        Some(state)
    }

    pub(crate) fn rebuild_preview(&mut self) {
        self.preview = build_batch_rename_preview(
            &self.items,
            &self.sequence,
            &self.replace,
            &self.insert,
            &self.slice,
            self.case,
            &self.existing_paths,
        );
    }

    pub(crate) fn can_apply(&self) -> bool {
        let has_problem = self.preview.rows.iter().any(|row| {
            matches!(
                row.status,
                BatchRenamePreviewStatus::EmptyName
                    | BatchRenamePreviewStatus::DuplicateTarget
                    | BatchRenamePreviewStatus::ExistingTarget
            )
        });
        let has_change = self
            .preview
            .rows
            .iter()
            .any(|row| row.status == BatchRenamePreviewStatus::Ready);
        !has_problem && has_change
    }

    pub(crate) fn plan(&self) -> Option<Vec<BatchRenameItem>> {
        self.can_apply().then(|| {
            self.preview
                .rows
                .iter()
                .map(|row| BatchRenameItem {
                    from: row.source.clone(),
                    to: row.target.clone(),
                })
                .collect()
        })
    }
}

impl Default for BatchRenameSequenceRule {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            start_input: "1".to_owned(),
            padding_input: "2".to_owned(),
            include_original_stem: true,
            preserve_extension: true,
        }
    }
}

impl Default for BatchRenameReplaceRule {
    fn default() -> Self {
        Self {
            find: String::new(),
            replacement: String::new(),
        }
    }
}

impl Default for BatchRenameInsertRule {
    fn default() -> Self {
        Self {
            text: String::new(),
            position_input: "0".to_owned(),
        }
    }
}

impl Default for BatchRenameSliceRule {
    fn default() -> Self {
        Self {
            start_input: String::new(),
            length_input: String::new(),
        }
    }
}

impl BatchRenameCaseRule {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unchanged => "Keep case",
            Self::Lowercase => "lowercase",
            Self::Uppercase => "UPPERCASE",
            Self::TitleCase => "Title Case",
        }
    }
}

impl BatchRenamePreviewStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Unchanged => "Unchanged",
            Self::EmptyName => "Empty name",
            Self::DuplicateTarget => "Duplicate target",
            Self::ExistingTarget => "Already exists",
        }
    }
}

fn build_batch_rename_preview(
    items: &[BatchRenameSource],
    sequence: &BatchRenameSequenceRule,
    replace: &BatchRenameReplaceRule,
    insert: &BatchRenameInsertRule,
    slice: &BatchRenameSliceRule,
    case: BatchRenameCaseRule,
    existing_paths: &HashSet<PathBuf>,
) -> BatchRenamePreview {
    let start = parse_usize_or_default(&sequence.start_input, 1);
    let padding = parse_usize_or_default(&sequence.padding_input, 0);
    let insert_position = parse_usize_or_default(&insert.position_input, 0);
    let slice_start = parse_optional_usize(&slice.start_input);
    let slice_length = parse_optional_usize(&slice.length_input);

    let mut rows = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let target_name = rename_item_name(
            item,
            start + index,
            padding,
            sequence,
            replace,
            insert,
            insert_position,
            slice_start,
            slice_length,
            case,
        );
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
            status: BatchRenamePreviewStatus::Ready,
        });
    }

    mark_batch_rename_preview_statuses(&mut rows, existing_paths);
    BatchRenamePreview { rows }
}

#[allow(clippy::too_many_arguments)]
fn rename_item_name(
    item: &BatchRenameSource,
    sequence_number: usize,
    padding: usize,
    sequence: &BatchRenameSequenceRule,
    replace: &BatchRenameReplaceRule,
    insert: &BatchRenameInsertRule,
    insert_position: usize,
    slice_start: Option<usize>,
    slice_length: Option<usize>,
    case: BatchRenameCaseRule,
) -> String {
    let (mut stem, extension) = split_file_name(&item.name);

    if !replace.find.is_empty() {
        stem = stem.replace(&replace.find, &replace.replacement);
    }
    if !insert.text.is_empty() {
        stem = insert_text_at_char_position(&stem, &insert.text, insert_position);
    }
    if slice_start.is_some() || slice_length.is_some() {
        stem = slice_text_by_chars(&stem, slice_start.unwrap_or(0), slice_length);
    }
    stem = apply_case_rule(&stem, case);

    let numbered = if sequence.prefix.is_empty() && sequence.include_original_stem {
        stem
    } else {
        let number = padded_number(sequence_number, padding);
        let mut name = format!("{}{}", sequence.prefix, number);
        if sequence.include_original_stem && !stem.is_empty() {
            name.push(' ');
            name.push_str(&stem);
        }
        name
    };

    if sequence.preserve_extension {
        join_stem_extension(&numbered, extension)
    } else {
        numbered
    }
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
        row.status = if row.target_name.is_empty() {
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

fn split_file_name(name: &str) -> (String, Option<&str>) {
    let path = Path::new(name);
    let extension = path.extension().and_then(|extension| extension.to_str());
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name)
        .to_owned();
    (stem, extension)
}

fn join_stem_extension(stem: &str, extension: Option<&str>) -> String {
    match extension {
        Some(extension) if !extension.is_empty() => format!("{stem}.{extension}"),
        _ => stem.to_owned(),
    }
}

fn insert_text_at_char_position(source: &str, text: &str, position: usize) -> String {
    let byte_position = source
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(source.len()))
        .nth(position)
        .unwrap_or(source.len());
    let mut next = String::with_capacity(source.len() + text.len());
    next.push_str(&source[..byte_position]);
    next.push_str(text);
    next.push_str(&source[byte_position..]);
    next
}

fn slice_text_by_chars(source: &str, start: usize, length: Option<usize>) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    if start >= chars.len() {
        return String::new();
    }
    let end = length
        .map(|length| start.saturating_add(length).min(chars.len()))
        .unwrap_or(chars.len());
    chars[start..end].iter().collect()
}

fn apply_case_rule(source: &str, case: BatchRenameCaseRule) -> String {
    match case {
        BatchRenameCaseRule::Unchanged => source.to_owned(),
        BatchRenameCaseRule::Lowercase => source.to_lowercase(),
        BatchRenameCaseRule::Uppercase => source.to_uppercase(),
        BatchRenameCaseRule::TitleCase => title_case(source),
    }
}

fn title_case(source: &str) -> String {
    let mut next_word = true;
    let mut output = String::new();
    for character in source.chars() {
        if character.is_alphanumeric() {
            if next_word {
                output.extend(character.to_uppercase());
                next_word = false;
            } else {
                output.extend(character.to_lowercase());
            }
        } else {
            output.push(character);
            next_word = true;
        }
    }
    output
}

fn padded_number(number: usize, padding: usize) -> String {
    if padding == 0 {
        number.to_string()
    } else {
        format!("{number:0padding$}")
    }
}

fn parse_usize_or_default(input: &str, default: usize) -> usize {
    input.trim().parse::<usize>().unwrap_or(default)
}

fn parse_optional_usize(input: &str) -> Option<usize> {
    let input = input.trim();
    if input.is_empty() {
        None
    } else {
        input.parse::<usize>().ok()
    }
}

pub(crate) fn same_parent(paths: &[PathBuf]) -> bool {
    let mut parents = paths.iter().filter_map(|path| path.parent());
    let Some(parent) = parents.next() else {
        return false;
    };
    parents.all(|candidate| candidate == parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_for_names(names: &[&str]) -> BatchRenameState {
        BatchRenameState::new_with_existing_paths(
            names
                .iter()
                .map(|name| PathBuf::from("/tmp").join(name))
                .collect(),
            HashSet::new(),
        )
        .unwrap()
    }

    #[test]
    fn batch_rename_sequence_prefixes_number_and_preserves_extension() {
        let mut state = state_for_names(&["report.txt", "notes.txt"]);
        state.sequence.prefix = "File ".to_owned();
        state.sequence.start_input = "3".to_owned();
        state.sequence.padding_input = "3".to_owned();
        state.rebuild_preview();

        let names = state
            .preview
            .rows
            .iter()
            .map(|row| row.target_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["File 003 report.txt", "File 004 notes.txt"]);
    }

    #[test]
    fn batch_rename_replace_insert_slice_and_case_are_ordered() {
        let mut state = state_for_names(&["summer draft.txt", "winter draft.txt"]);
        state.replace.find = "draft".to_owned();
        state.replace.replacement = "photo".to_owned();
        state.insert.text = "2026 ".to_owned();
        state.insert.position_input = "0".to_owned();
        state.slice.start_input = "0".to_owned();
        state.slice.length_input = "11".to_owned();
        state.case = BatchRenameCaseRule::Uppercase;
        state.rebuild_preview();

        let names = state
            .preview
            .rows
            .iter()
            .map(|row| row.target_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["2026 SUMMER.txt", "2026 WINTER.txt"]);
    }

    #[test]
    fn batch_rename_preview_marks_duplicate_targets() {
        let mut state = state_for_names(&["a.txt", "b.txt"]);
        state.slice.start_input = "99".to_owned();
        state.rebuild_preview();

        assert!(state
            .preview
            .rows
            .iter()
            .all(|row| row.status == BatchRenamePreviewStatus::DuplicateTarget));
        assert!(!state.can_apply());
    }

    #[test]
    fn batch_rename_preview_marks_existing_unselected_target() {
        let existing = [PathBuf::from("/tmp/taken.txt")]
            .into_iter()
            .collect::<HashSet<_>>();
        let mut state = BatchRenameState::new_with_existing_paths(
            vec![
                PathBuf::from("/tmp/report.txt"),
                PathBuf::from("/tmp/notes.txt"),
            ],
            existing,
        )
        .unwrap();
        state.replace.find = "report".to_owned();
        state.replace.replacement = "taken".to_owned();
        state.rebuild_preview();

        assert_eq!(
            state.preview.rows[0].status,
            BatchRenamePreviewStatus::ExistingTarget
        );
        assert!(!state.can_apply());
    }
}
