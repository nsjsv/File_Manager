use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use file_core::BatchRenameItem;

mod transforms;
use transforms::PreparedBatchRenameRules;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub(crate) enum BatchRenameMessage {
    OpenSelected,
    RulePanelSelected(BatchRenameRulePanel),
    SortModeSelected(BatchRenameSortMode),
    ExtensionModeSelected(BatchRenameExtensionMode),
    ExtensionReplacementChanged(String),
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
    RandomModeSelected(BatchRenameRandomMode),
    RandomLengthChanged(String),
    RandomAlphabetChanged(String),
    RemoveTextChanged(String),
    RemoveStartChanged(String),
    RemoveLengthChanged(String),
    ListNamesChanged(String),
    CustomTemplateChanged(String),
    RegexPatternChanged(String),
    RegexReplacementChanged(String),
    BatchCommandsChanged(String),
    PreviewDragStarted(PathBuf),
    PreviewDragEntered(PathBuf),
    PreviewDragFinished,
    Apply,
    Cancel,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchRenameState {
    pub(crate) items: Vec<BatchRenameSource>,
    pub(crate) active_panel: BatchRenameRulePanel,
    pub(crate) sort: BatchRenameSortRule,
    pub(crate) extension: BatchRenameExtensionRule,
    pub(crate) sequence: BatchRenameSequenceRule,
    pub(crate) replace: BatchRenameReplaceRule,
    pub(crate) insert: BatchRenameInsertRule,
    pub(crate) slice: BatchRenameSliceRule,
    pub(crate) case: BatchRenameCaseRule,
    pub(crate) random: BatchRenameRandomRule,
    pub(crate) remove: BatchRenameRemoveRule,
    pub(crate) list: BatchRenameListRule,
    pub(crate) custom: BatchRenameCustomRule,
    pub(crate) regex: BatchRenameRegexRule,
    pub(crate) batch: BatchRenameBatchRule,
    dragging_preview_source: Option<PathBuf>,
    existing_paths: HashSet<PathBuf>,
    pub(crate) preview: BatchRenamePreview,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchRenameSource {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameRulePanel {
    Sort,
    Extension,
    Case,
    Sequence,
    Replace,
    Insert,
    Slice,
    Random,
    Remove,
    List,
    Custom,
    Regex,
    Batch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameSortRule {
    pub(crate) mode: BatchRenameSortMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameSortMode {
    SelectionOrder,
    NameAscending,
    NameDescending,
    ExtensionAscending,
    ExtensionDescending,
    Reverse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameExtensionRule {
    pub(crate) mode: BatchRenameExtensionMode,
    pub(crate) replacement: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameExtensionMode {
    Preserve,
    Remove,
    Replace,
    Lowercase,
    Uppercase,
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
pub(crate) struct BatchRenameRandomRule {
    pub(crate) mode: BatchRenameRandomMode,
    pub(crate) length_input: String,
    pub(crate) alphabet: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameRandomMode {
    Off,
    ReplaceStem,
    Prefix,
    Suffix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameRemoveRule {
    pub(crate) text: String,
    pub(crate) start_input: String,
    pub(crate) length_input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameListRule {
    pub(crate) names: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameCustomRule {
    pub(crate) template: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameRegexRule {
    pub(crate) pattern: String,
    pub(crate) replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameBatchRule {
    pub(crate) commands: String,
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
    RuleError,
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
            active_panel: BatchRenameRulePanel::Sequence,
            sort: BatchRenameSortRule::default(),
            extension: BatchRenameExtensionRule::default(),
            sequence: BatchRenameSequenceRule::default(),
            replace: BatchRenameReplaceRule::default(),
            insert: BatchRenameInsertRule::default(),
            slice: BatchRenameSliceRule::default(),
            case: BatchRenameCaseRule::Unchanged,
            random: BatchRenameRandomRule::default(),
            remove: BatchRenameRemoveRule::default(),
            list: BatchRenameListRule::default(),
            custom: BatchRenameCustomRule::default(),
            regex: BatchRenameRegexRule::default(),
            batch: BatchRenameBatchRule::default(),
            dragging_preview_source: None,
            existing_paths,
            preview: BatchRenamePreview { rows: Vec::new() },
        };
        state.rebuild_preview();
        Some(state)
    }

    pub(crate) fn rebuild_preview(&mut self) {
        self.preview = build_batch_rename_preview(self);
    }

    pub(crate) fn apply_update(&mut self, message: BatchRenameMessage) {
        match message {
            BatchRenameMessage::RulePanelSelected(panel) => self.active_panel = panel,
            BatchRenameMessage::SortModeSelected(mode) => self.sort.mode = mode,
            BatchRenameMessage::ExtensionModeSelected(mode) => self.extension.mode = mode,
            BatchRenameMessage::ExtensionReplacementChanged(value) => {
                self.extension.replacement = value
            }
            BatchRenameMessage::SequencePrefixChanged(value) => self.sequence.prefix = value,
            BatchRenameMessage::SequenceStartChanged(value) => self.sequence.start_input = value,
            BatchRenameMessage::SequencePaddingChanged(value) => {
                self.sequence.padding_input = value
            }
            BatchRenameMessage::SequenceIncludeOriginalToggled(value) => {
                self.sequence.include_original_stem = value
            }
            BatchRenameMessage::SequencePreserveExtensionToggled(value) => {
                self.sequence.preserve_extension = value
            }
            BatchRenameMessage::ReplaceFindChanged(value) => self.replace.find = value,
            BatchRenameMessage::ReplaceWithChanged(value) => self.replace.replacement = value,
            BatchRenameMessage::InsertTextChanged(value) => self.insert.text = value,
            BatchRenameMessage::InsertPositionChanged(value) => self.insert.position_input = value,
            BatchRenameMessage::SliceStartChanged(value) => self.slice.start_input = value,
            BatchRenameMessage::SliceLengthChanged(value) => self.slice.length_input = value,
            BatchRenameMessage::CaseSelected(case) => self.case = case,
            BatchRenameMessage::RandomModeSelected(mode) => self.random.mode = mode,
            BatchRenameMessage::RandomLengthChanged(value) => self.random.length_input = value,
            BatchRenameMessage::RandomAlphabetChanged(value) => self.random.alphabet = value,
            BatchRenameMessage::RemoveTextChanged(value) => self.remove.text = value,
            BatchRenameMessage::RemoveStartChanged(value) => self.remove.start_input = value,
            BatchRenameMessage::RemoveLengthChanged(value) => self.remove.length_input = value,
            BatchRenameMessage::ListNamesChanged(value) => self.list.names = value,
            BatchRenameMessage::CustomTemplateChanged(value) => self.custom.template = value,
            BatchRenameMessage::RegexPatternChanged(value) => self.regex.pattern = value,
            BatchRenameMessage::RegexReplacementChanged(value) => self.regex.replacement = value,
            BatchRenameMessage::BatchCommandsChanged(value) => self.batch.commands = value,
            BatchRenameMessage::PreviewDragStarted(source) => self.start_preview_drag(source),
            BatchRenameMessage::PreviewDragEntered(target) => {
                self.reorder_dragging_preview_source(target)
            }
            BatchRenameMessage::PreviewDragFinished => self.finish_preview_drag(),
            BatchRenameMessage::OpenSelected
            | BatchRenameMessage::Apply
            | BatchRenameMessage::Cancel => {}
        }
    }

    pub(crate) fn dragging_preview_source(&self) -> Option<&Path> {
        self.dragging_preview_source.as_deref()
    }

    pub(crate) fn finish_preview_drag(&mut self) {
        self.dragging_preview_source = None;
    }

    pub(crate) fn can_apply(&self) -> bool {
        let has_problem = self.preview.rows.iter().any(|row| {
            matches!(
                row.status,
                BatchRenamePreviewStatus::EmptyName
                    | BatchRenamePreviewStatus::DuplicateTarget
                    | BatchRenamePreviewStatus::ExistingTarget
                    | BatchRenamePreviewStatus::RuleError
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

    fn start_preview_drag(&mut self, source: PathBuf) {
        if self.items.iter().any(|item| item.path == source) {
            self.dragging_preview_source = Some(source);
        }
    }

    fn reorder_dragging_preview_source(&mut self, target: PathBuf) {
        let Some(source) = self.dragging_preview_source.as_ref() else {
            return;
        };
        if *source == target {
            return;
        }

        let Some(source_index) = self.items.iter().position(|item| item.path == *source) else {
            self.dragging_preview_source = None;
            return;
        };
        let Some(target_index) = self.items.iter().position(|item| item.path == target) else {
            return;
        };

        let item = self.items.remove(source_index);
        let insertion_index = target_index.min(self.items.len());
        self.items.insert(insertion_index, item);
        self.sort.mode = BatchRenameSortMode::SelectionOrder;
    }
}

impl BatchRenameRulePanel {
    pub(crate) fn options() -> Vec<Self> {
        vec![
            Self::Sort,
            Self::Extension,
            Self::Case,
            Self::Sequence,
            Self::Replace,
            Self::Insert,
            Self::Slice,
            Self::Random,
            Self::Remove,
            Self::List,
            Self::Custom,
            Self::Regex,
            Self::Batch,
        ]
    }
}

impl fmt::Display for BatchRenameRulePanel {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self {
            Self::Sort => "Sort",
            Self::Extension => "Extension",
            Self::Case => "Case",
            Self::Sequence => "Sequence",
            Self::Replace => "Replace",
            Self::Insert => "Insert",
            Self::Slice => "Slice",
            Self::Random => "Random",
            Self::Remove => "Remove",
            Self::List => "List",
            Self::Custom => "Custom",
            Self::Regex => "Regex",
            Self::Batch => "Batch",
        })
    }
}

impl Default for BatchRenameSortRule {
    fn default() -> Self {
        Self {
            mode: BatchRenameSortMode::SelectionOrder,
        }
    }
}

impl BatchRenameSortMode {
    pub(crate) fn options() -> Vec<Self> {
        vec![
            Self::SelectionOrder,
            Self::NameAscending,
            Self::NameDescending,
            Self::ExtensionAscending,
            Self::ExtensionDescending,
            Self::Reverse,
        ]
    }
}

impl fmt::Display for BatchRenameSortMode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self {
            Self::SelectionOrder => "Selection order",
            Self::NameAscending => "Name A-Z",
            Self::NameDescending => "Name Z-A",
            Self::ExtensionAscending => "Extension A-Z",
            Self::ExtensionDescending => "Extension Z-A",
            Self::Reverse => "Reverse",
        })
    }
}

impl Default for BatchRenameExtensionRule {
    fn default() -> Self {
        Self {
            mode: BatchRenameExtensionMode::Preserve,
            replacement: String::new(),
        }
    }
}

impl BatchRenameExtensionMode {
    pub(crate) fn options() -> Vec<Self> {
        vec![
            Self::Preserve,
            Self::Remove,
            Self::Replace,
            Self::Lowercase,
            Self::Uppercase,
        ]
    }
}

impl fmt::Display for BatchRenameExtensionMode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self {
            Self::Preserve => "Preserve",
            Self::Remove => "Remove",
            Self::Replace => "Replace",
            Self::Lowercase => "lowercase",
            Self::Uppercase => "UPPERCASE",
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

impl Default for BatchRenameRandomRule {
    fn default() -> Self {
        Self {
            mode: BatchRenameRandomMode::Off,
            length_input: "6".to_owned(),
            alphabet: "abcdefghijklmnopqrstuvwxyz0123456789".to_owned(),
        }
    }
}

impl BatchRenameRandomMode {
    pub(crate) fn options() -> Vec<Self> {
        vec![Self::Off, Self::ReplaceStem, Self::Prefix, Self::Suffix]
    }
}

impl fmt::Display for BatchRenameRandomMode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(match self {
            Self::Off => "Off",
            Self::ReplaceStem => "Replace stem",
            Self::Prefix => "Prefix",
            Self::Suffix => "Suffix",
        })
    }
}

impl Default for BatchRenameRemoveRule {
    fn default() -> Self {
        Self {
            text: String::new(),
            start_input: String::new(),
            length_input: String::new(),
        }
    }
}

impl Default for BatchRenameListRule {
    fn default() -> Self {
        Self {
            names: String::new(),
        }
    }
}

impl Default for BatchRenameCustomRule {
    fn default() -> Self {
        Self {
            template: String::new(),
        }
    }
}

impl Default for BatchRenameRegexRule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            replacement: String::new(),
        }
    }
}

impl Default for BatchRenameBatchRule {
    fn default() -> Self {
        Self {
            commands: String::new(),
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
            Self::RuleError => "Rule error",
        }
    }
}

fn build_batch_rename_preview(state: &BatchRenameState) -> BatchRenamePreview {
    let sorted_items = sorted_batch_rename_items(&state.items, &state.sort);
    let prepared_rules = PreparedBatchRenameRules::new(state);
    let mut rows = Vec::with_capacity(sorted_items.len());

    for (index, item) in sorted_items.into_iter().enumerate() {
        let target_name_result = prepared_rules.rename_item_name(item, index, state);
        let has_rule_error = target_name_result.is_err();
        let target_name = target_name_result.unwrap_or_else(|_| item.name.clone());
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
        BatchRenameSortMode::NameAscending => {
            sorted.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        }
        BatchRenameSortMode::NameDescending => {
            sorted.sort_by(|left, right| right.name.to_lowercase().cmp(&left.name.to_lowercase()));
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

pub(crate) fn same_parent(paths: &[PathBuf]) -> bool {
    let mut parents = paths.iter().filter_map(|path| path.parent());
    let Some(parent) = parents.next() else {
        return false;
    };
    parents.all(|candidate| candidate == parent)
}
