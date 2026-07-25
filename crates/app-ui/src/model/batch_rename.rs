use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use file_core::{BatchRenameItem, DirectoryEntry};

mod preview;
mod rule_options;
mod transforms;
use preview::build_batch_rename_preview;

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
    SequenceStepChanged(String),
    SequencePaddingChanged(String),
    SequenceIncludeOriginalToggled(bool),
    SequencePreserveExtensionToggled(bool),
    ReplaceFindChanged(String),
    ReplaceWithChanged(String),
    ReplaceScopeSelected(BatchRenameReplaceScope),
    ReplaceRangeStartChanged(String),
    ReplaceRangeLengthChanged(String),
    ReplaceIgnoreCaseToggled(bool),
    InsertTextChanged(String),
    InsertPositionChanged(String),
    InsertModeSelected(BatchRenameInsertMode),
    InsertAnchorChanged(String),
    InsertIgnoreExtensionToggled(bool),
    SliceStartChanged(String),
    SliceLengthChanged(String),
    SliceModeSelected(BatchRenameSliceMode),
    SliceAnchorChanged(String),
    CaseSelected(BatchRenameCaseRule),
    RandomModeSelected(BatchRenameRandomMode),
    RandomLengthChanged(String),
    RandomAlphabetChanged(String),
    RemoveTextChanged(String),
    RemoveStartChanged(String),
    RemoveLengthChanged(String),
    RemoveModeSelected(BatchRenameRemoveMode),
    RemoveClassToggled(BatchRenameRemoveClass, bool),
    ListNamesChanged(String),
    CustomTemplateChanged(String),
    RegexPatternChanged(String),
    RegexReplacementChanged(String),
    BatchCommandsChanged(String),
    PreviewNameEditStarted(PathBuf),
    PreviewNameChanged(String),
    PreviewNameEditCommitted,
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
    manual_target_name_overrides: HashMap<PathBuf, String>,
    editing_target_name_source: Option<PathBuf>,
    editing_target_name_input: String,
    dragging_preview_source: Option<PathBuf>,
    existing_paths: HashSet<PathBuf>,
    pub(crate) preview: BatchRenamePreview,
}

#[derive(Debug, Clone)]
pub(crate) struct BatchRenameSource {
    pub(crate) path: PathBuf,
    pub(crate) source_name_text: String,
    pub(crate) modified: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameSourceNameError {
    pub(crate) path: PathBuf,
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
    NaturalAscending,
    NameAscending,
    NameDescending,
    ModifiedAscending,
    ModifiedDescending,
    Random,
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
    pub(crate) step_input: String,
    pub(crate) padding_input: String,
    pub(crate) include_original_stem: bool,
    pub(crate) preserve_extension: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameReplaceRule {
    pub(crate) find: String,
    pub(crate) replacement: String,
    pub(crate) scope: BatchRenameReplaceScope,
    pub(crate) range_start_input: String,
    pub(crate) range_length_input: String,
    pub(crate) ignore_case: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameReplaceScope {
    All,
    First,
    Last,
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameInsertRule {
    pub(crate) text: String,
    pub(crate) mode: BatchRenameInsertMode,
    pub(crate) position_input: String,
    pub(crate) anchor: String,
    pub(crate) ignore_extension: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameInsertMode {
    Before,
    After,
    Position,
    AfterAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BatchRenameSliceRule {
    pub(crate) mode: BatchRenameSliceMode,
    pub(crate) start_input: String,
    pub(crate) length_input: String,
    pub(crate) anchor: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameSliceMode {
    Position,
    AfterAnchor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameCaseRule {
    Unchanged,
    Lowercase,
    Uppercase,
    TitleCase,
    InvertCase,
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
    pub(crate) mode: BatchRenameRemoveMode,
    pub(crate) start_input: String,
    pub(crate) length_input: String,
    pub(crate) classes: Vec<BatchRenameRemoveClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameRemoveMode {
    TextAndRange,
    CharacterClasses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchRenameRemoveClass {
    Lowercase,
    Uppercase,
    Digits,
    Symbols,
    Brackets,
    Whitespace,
    Hanzi,
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
    pub(crate) fn new_with_existing_sources(
        items: Vec<BatchRenameSource>,
        existing_paths: HashSet<PathBuf>,
    ) -> Option<Self> {
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
            manual_target_name_overrides: HashMap::new(),
            editing_target_name_source: None,
            editing_target_name_input: String::new(),
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
            BatchRenameMessage::SequenceStepChanged(value) => self.sequence.step_input = value,
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
            BatchRenameMessage::ReplaceScopeSelected(value) => self.replace.scope = value,
            BatchRenameMessage::ReplaceRangeStartChanged(value) => {
                self.replace.range_start_input = value
            }
            BatchRenameMessage::ReplaceRangeLengthChanged(value) => {
                self.replace.range_length_input = value
            }
            BatchRenameMessage::ReplaceIgnoreCaseToggled(value) => self.replace.ignore_case = value,
            BatchRenameMessage::InsertTextChanged(value) => self.insert.text = value,
            BatchRenameMessage::InsertPositionChanged(value) => self.insert.position_input = value,
            BatchRenameMessage::InsertModeSelected(value) => self.insert.mode = value,
            BatchRenameMessage::InsertAnchorChanged(value) => self.insert.anchor = value,
            BatchRenameMessage::InsertIgnoreExtensionToggled(value) => {
                self.insert.ignore_extension = value
            }
            BatchRenameMessage::SliceStartChanged(value) => self.slice.start_input = value,
            BatchRenameMessage::SliceLengthChanged(value) => self.slice.length_input = value,
            BatchRenameMessage::SliceModeSelected(value) => self.slice.mode = value,
            BatchRenameMessage::SliceAnchorChanged(value) => self.slice.anchor = value,
            BatchRenameMessage::CaseSelected(case) => self.case = case,
            BatchRenameMessage::RandomModeSelected(mode) => self.random.mode = mode,
            BatchRenameMessage::RandomLengthChanged(value) => self.random.length_input = value,
            BatchRenameMessage::RandomAlphabetChanged(value) => self.random.alphabet = value,
            BatchRenameMessage::RemoveTextChanged(value) => self.remove.text = value,
            BatchRenameMessage::RemoveStartChanged(value) => self.remove.start_input = value,
            BatchRenameMessage::RemoveLengthChanged(value) => self.remove.length_input = value,
            BatchRenameMessage::RemoveModeSelected(value) => self.remove.mode = value,
            BatchRenameMessage::RemoveClassToggled(class, enabled) => {
                self.update_remove_class(class, enabled)
            }
            BatchRenameMessage::ListNamesChanged(value) => self.list.names = value,
            BatchRenameMessage::CustomTemplateChanged(value) => self.custom.template = value,
            BatchRenameMessage::RegexPatternChanged(value) => self.regex.pattern = value,
            BatchRenameMessage::RegexReplacementChanged(value) => self.regex.replacement = value,
            BatchRenameMessage::BatchCommandsChanged(value) => self.batch.commands = value,
            BatchRenameMessage::PreviewNameEditStarted(source) => {
                self.start_preview_name_edit(source)
            }
            BatchRenameMessage::PreviewNameChanged(value) => self.update_preview_name_edit(value),
            BatchRenameMessage::PreviewNameEditCommitted => self.finish_preview_name_edit(),
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

    pub(crate) fn editing_target_name_source(&self) -> Option<&Path> {
        self.editing_target_name_source.as_deref()
    }

    pub(crate) fn editing_target_name_input(&self) -> &str {
        &self.editing_target_name_input
    }

    pub(crate) fn preview_target_name_for_source(&self, source: &Path) -> Option<&str> {
        self.preview
            .rows
            .iter()
            .find(|row| row.source == source)
            .map(|row| row.target_name.as_str())
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

    fn start_preview_name_edit(&mut self, source: PathBuf) {
        let Some(target_name) = self
            .preview_target_name_for_source(&source)
            .map(ToOwned::to_owned)
        else {
            return;
        };
        self.editing_target_name_source = Some(source);
        self.editing_target_name_input = target_name;
    }

    fn update_preview_name_edit(&mut self, value: String) {
        let Some(source) = self.editing_target_name_source.clone() else {
            return;
        };
        self.editing_target_name_input = value.clone();
        self.manual_target_name_overrides.insert(source, value);
    }

    fn finish_preview_name_edit(&mut self) {
        self.editing_target_name_source = None;
        self.editing_target_name_input.clear();
    }

    fn update_remove_class(&mut self, class: BatchRenameRemoveClass, enabled: bool) {
        if enabled {
            if !self.remove.classes.contains(&class) {
                self.remove.classes.push(class);
            }
            return;
        }
        self.remove.classes.retain(|candidate| *candidate != class);
    }

    pub(super) fn manual_target_name_override(&self, source: &Path) -> Option<&str> {
        self.manual_target_name_overrides
            .get(source)
            .map(String::as_str)
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

impl BatchRenameSource {
    pub(crate) fn try_from_entry(
        entry: &DirectoryEntry,
    ) -> Result<Self, BatchRenameSourceNameError> {
        let source_name_text = entry
            .name()
            .to_str()
            .ok_or_else(|| BatchRenameSourceNameError {
                path: entry.path.clone(),
            })?;
        Ok(Self {
            path: entry.path.clone(),
            source_name_text: source_name_text.to_owned(),
            modified: entry.metadata.modified,
        })
    }
}

impl fmt::Display for BatchRenameSourceNameError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "Batch rename does not support non-UTF-8 file names: {:?}",
            self.path
        )
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

pub(crate) fn same_parent(paths: &[PathBuf]) -> bool {
    let mut parents = paths.iter().filter_map(|path| path.parent());
    let Some(parent) = parents.next() else {
        return false;
    };
    parents.all(|candidate| candidate == parent)
}
