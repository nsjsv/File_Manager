use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use desktop_linux::{DesktopClipboardContent, TerminalEmulator};
use file_core::{
    DirectoryEntry, DirectoryScan, FileKind, FileSearchIndexOutcome, FileSearchMatch,
    FileSearchOutcome, TrashEntry, TrashRestoreEntry, TrashScan,
};
use file_operation_store::TaskQueueStore;
use iced::keyboard;
use iced::widget::{image, text_editor};
use iced::{event, mouse, window, Point, Rectangle, Theme};

use crate::audio_preview::AudioPreviewRuntime;
use crate::config::{RenderingGpuPreference, UserConfig};
use crate::operation_history::FileOperationOutcome;
use crate::operation_queue::{FileOperationProgressUpdate, QueuedTransfer};
use crate::thumbnail_cache::{ColumnViewport, ThumbnailLoadOutcome};

pub(crate) use file_core::{TransferConflictItem, TransferConflictMetadata};

#[derive(Debug, Clone)]
pub(crate) struct LoadedOperationStore {
    pub(crate) task_queue_store: TaskQueueStore,
    pub(crate) column_width_overrides: HashMap<usize, f32>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    StartupEnvironmentLoaded(StartupEnvironment),
    SidebarLocationsLoaded(Vec<SidebarLocation>),
    OperationStoreLoaded(Result<LoadedOperationStore, String>),
    Loaded(BrowserPaneId, Result<DirectoryScan, String>),
    TrashLoaded(BrowserPaneId, Result<TrashScan, String>),
    OpenFileFinished(Result<(), String>),
    OpenTerminalFinished(Result<(), String>),
    PreviewLoaded(PathBuf, Result<PreviewContent, String>),
    PreviewDirectoryChildrenLoaded(PathBuf, Result<Vec<DirectoryEntry>, String>),
    TextPreviewAction(text_editor::Action),
    MarkdownPreviewModeSelected(MarkdownPreviewMode),
    ImagePreviewDimensionsLoaded(PathBuf, Result<(u32, u32), String>),
    AudioPreviewPlaybackToggled,
    AudioPreviewStarted(PathBuf, Result<AudioPreviewRuntime, String>),
    AudioPreviewSeekRequested(f32),
    AudioPreviewVolumeChanged(f32),
    AudioPreviewTick,
    VideoPreviewPlaybackToggled,
    VideoPreviewAudioStarted(PathBuf, u64, Result<AudioPreviewRuntime, String>),
    VideoPreviewMetadataLoaded(PathBuf, Result<Option<Duration>, String>),
    VideoPreviewSeekRequested(f32),
    VideoPreviewSeekCommitted,
    VideoPreviewVolumeChanged(f32),
    VideoPreviewTick,
    VideoPreviewFrameLoaded(VideoPreviewFrame),
    VideoPreviewSeekFrameFailed(PathBuf, u64, Duration, String),
    VideoPreviewFinished(PathBuf, u64),
    VideoPreviewFailed(PathBuf, u64, String),
    FileOperationProgressed(u64, FileOperationProgressUpdate),
    FileOperationFinished(u64, Result<FileOperationOutcome, String>),
    FileOperationIndicatorPressed,
    FileOperationAutoHideElapsed(u64),
    FileOperationPauseToggled(u64),
    FileOperationCancelRequested(u64),
    PreviewTreeDirectoryToggled(usize),
    PreviewTreeAnimationTick,
    ThumbnailRefreshRequested(BrowserPaneId, PathBuf),
    ThumbnailBatchLoaded(Vec<ThumbnailLoadOutcome>),
    ColumnEntryClicked(BrowserPaneId, PathBuf),
    ColumnBlankClicked(BrowserPaneId, PathBuf),
    EntryReleased(BrowserPaneId),
    EntryRightClicked(BrowserPaneId, PathBuf),
    EntryHovered(BrowserPaneId, PathBuf),
    EntryHoverCleared(BrowserPaneId, PathBuf),
    DropTargetHovered(BrowserPaneId, PathBuf),
    DropTargetHoverCleared(BrowserPaneId, PathBuf),
    BlankAreaPressed(BrowserPaneId),
    BlankAreaRightClicked(BrowserPaneId, PathBuf),
    SidebarHovered(PathBuf),
    SidebarHoverCleared(PathBuf),
    SidebarPointerMoved(Point),
    SidebarPointerExited,
    SidebarBookmarkDropSlotHovered(SidebarBookmarkDropSlot),
    SidebarBookmarkDropSlotCleared(SidebarBookmarkDropSlot),
    SidebarBookmarkPressed(PathBuf),
    SidebarBookmarkRightClicked(PathBuf),
    SidebarBookmarkEntered(PathBuf),
    SidebarBookmarkReleased,
    SidebarBookmarkDeleteRequested(PathBuf),
    SidebarResizeStarted,
    CursorMoved(Point),
    ColumnBrowserCursorEntered(BrowserPaneId),
    ColumnBrowserCursorExited(BrowserPaneId),
    ColumnEntryBoundsMeasured(Vec<ColumnEntryBounds>),
    PaneCursorEntered(BrowserPaneId),
    PaneCursorExited(BrowserPaneId),
    KeyboardModifiersChanged(keyboard::Modifiers),
    DragSelectionFinished,
    DismissFloating,
    DestructiveActionConfirmed,
    DestructiveActionCanceled,
    AuxiliaryWindowCloseRequested(window::Id),
    AuxiliaryWindowResized(window::Id, f32, f32),
    WindowFocused(window::Id),
    WindowUnfocused(window::Id),
    FocusedWindowEscapePressed,
    WindowPointerPressed {
        button: mouse::Button,
        status: event::Status,
    },
    CapturedPreviewShortcutPressed,
    RequestPreview,
    PathInputChanged(BrowserPaneId, String),
    PathInputSubmitted(BrowserPaneId),
    PathSuggestionSelected(BrowserPaneId, PathBuf),
    PathSuggestionMoved(PathSuggestionDirection),
    PathSuggestionCompleted(PathSuggestionDirection),
    PathInputStabilized(BrowserPaneId, PathSuggestionRequest),
    PathSuggestionsLoaded(BrowserPaneId, PathSuggestionRequest, Vec<PathBuf>),
    SystemThemeDetected(Theme),
    UserConfigSaved(Result<(), String>),
    ColumnWidthOverrideSaved(Result<(), String>),
    SidebarBookmarksSaved(Result<(), String>),
    SearchOpened,
    SearchInputChanged(String),
    SearchInputStabilized(SearchRequest),
    SearchFocusRequested,
    SearchMatchesLoaded(SearchRequest, Result<FileSearchOutcome, String>),
    SearchIndexBuilt(PathBuf, Result<FileSearchIndexOutcome, String>),
    SearchMatchSelected(PathBuf),
    SearchActivated,
    ExpandedDirectoryLoaded(BrowserPaneId, PathBuf, Result<DirectoryScan, String>),
    ObservedDirectoryChanged(PathBuf),
    ColumnSettingsToggled,
    ShowHiddenFilesToggled,
    TerminalEmulatorSelected(TerminalEmulator),
    RenderingGpuPreferenceSelected(RenderingGpuPreference),
    RendererRestartNoticeDismissed,
    CapturedWheelScrolled(mouse::ScrollDelta),
    ScrollbarAutoHideElapsed(u64),
    WindowChromeAnimationTick,
    ColumnScrolled(BrowserPaneId, PathBuf, f32, f32),
    ColumnResizeStarted(BrowserPaneId, usize),
    OpenDirectoryInNewTab(BrowserPaneId, PathBuf),
    OpenTrashInNewTab(BrowserPaneId),
    TabPressed(BrowserPaneId, usize),
    TabCloseRequested(BrowserPaneId, usize),
    TabDragEntered(BrowserPaneId, usize),
    TabDragFinished,
    PaneBack(BrowserPaneId),
    PaneForward(BrowserPaneId),
    PaneUp(BrowserPaneId),
    NavigateTo(PathBuf),
    TrashOpened,
    Back,
    Forward,
    RenameInputFocusChecked(bool),
    RenameInputChanged(String),
    BeginRename(PathBuf),
    OpenTerminalHere(PathBuf),
    RenameSelected,
    UndoFileOperation,
    RedoFileOperation,
    CreateDirectory(PathBuf),
    CreateEmptyFile(PathBuf),
    TrashSelected,
    RestoreSelected,
    EmptyTrashRequested,
    CopySelected,
    MoveSelected,
    PastePending,
    FileClipboardWriteFinished(Result<(), String>),
    DesktopClipboardReadFinished {
        paste_directory: PathBuf,
        fallback_operation: Option<PendingOperation>,
        content: Result<Option<DesktopClipboardContent>, String>,
    },
    ClipboardFileCreated(Result<PathBuf, String>),
    PrimarySelectAllRequested,
    TransferConflictsChecked {
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
        conflicts: Vec<TransferConflictItem>,
    },
    TransferConflictChoiceSelected(TransferConflictChoice),
    TransferConflictApplyToAllToggled,
    TransferConflictRenameInputChanged(String),
    TransferConflictRenameConfirmed,
    TransferConflictRenameTargetChecked {
        state: TransferConflictState,
        transfer_position: Option<usize>,
        target: PathBuf,
        available: Result<bool, String>,
    },
    TransferConflictCancelRequested,
    SelectAll,
}

#[derive(Debug, Clone)]
pub(crate) enum PendingOperation {
    Copy(Vec<PathBuf>),
    Move(Vec<PathBuf>),
}

#[derive(Debug, Clone)]
pub(crate) enum DestructiveActionConfirmation {
    DeleteTrashEntries { entries: Vec<TrashRestoreEntry> },
    EmptyTrash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationQueuePanelMode {
    PassivePreview,
    InteractiveList,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ScrollbarVisibility {
    Hidden,
    VisibleWithOpacity(f32),
    Visible,
}

impl ScrollbarVisibility {
    pub(crate) fn with_opacity(opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        if opacity <= f32::EPSILON {
            Self::Hidden
        } else if (1.0 - opacity) <= f32::EPSILON {
            Self::Visible
        } else {
            Self::VisibleWithOpacity(opacity)
        }
    }

    pub(crate) fn opacity(self) -> f32 {
        match self {
            Self::Hidden => 0.0,
            Self::VisibleWithOpacity(opacity) => opacity,
            Self::Visible => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferConflictMode {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransferConflictChoice {
    Replace,
    Skip,
    KeepBoth,
    Merge,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferConflictState {
    pub(crate) mode: TransferConflictMode,
    pub(crate) transfers: Vec<QueuedTransfer>,
    pub(crate) conflicts: Vec<TransferConflictItem>,
    pub(crate) current_index: usize,
    pub(crate) apply_to_all: bool,
    pub(crate) rename_input: String,
}

impl TransferConflictState {
    pub(crate) fn current_conflict(&self) -> Option<&TransferConflictItem> {
        self.conflicts.get(self.current_index)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StartupEnvironment {
    pub(crate) home: PathBuf,
    pub(crate) user_config: UserConfig,
    pub(crate) state_database_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BrowserPaneId(pub(crate) u64);

impl BrowserPaneId {
    pub(crate) const PRIMARY: Self = Self(0);

    pub(crate) fn key(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitRegion {
    Left,
    Right,
    Top,
    Bottom,
}

impl SplitRegion {
    pub(crate) fn axis(self) -> SplitAxis {
        match self {
            Self::Left | Self::Right => SplitAxis::Horizontal,
            Self::Top | Self::Bottom => SplitAxis::Vertical,
        }
    }

    pub(crate) fn places_dragged_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserPaneLayout {
    Single {
        active: BrowserPaneId,
    },
    Split {
        axis: SplitAxis,
        first: BrowserPaneId,
        second: BrowserPaneId,
        active: BrowserPaneId,
    },
}

impl BrowserPaneLayout {
    pub(crate) fn active(self) -> BrowserPaneId {
        match self {
            Self::Single { active } | Self::Split { active, .. } => active,
        }
    }

    pub(crate) fn with_active(self, next_active: BrowserPaneId) -> Self {
        match self {
            Self::Single { .. } => Self::Single {
                active: next_active,
            },
            Self::Split {
                axis,
                first,
                second,
                ..
            } => Self::Split {
                axis,
                first,
                second,
                active: next_active,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserPane {
    pub(crate) id: BrowserPaneId,
    pub(crate) current_dir: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) selected_paths: HashSet<PathBuf>,
    pub(crate) selection_anchor: Option<PathBuf>,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) column_viewports: HashMap<PathBuf, ColumnViewport>,
    pub(crate) tabs: Vec<BrowserTab>,
    pub(crate) active_tab_id: usize,
    pub(crate) path_input: String,
    pub(crate) path_suggestions: Vec<PathBuf>,
    pub(crate) path_suggestion_selection: Option<usize>,
    pub(crate) path_suggestion_generation: u64,
    pub(crate) back_stack: Vec<PathBuf>,
    pub(crate) forward_stack: Vec<PathBuf>,
    pub(crate) is_loading: bool,
}

impl BrowserPane {
    pub(crate) fn sync_active_tab_state(&mut self) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab_id)
        else {
            return;
        };

        tab.directory = self.current_dir.clone();
        tab.is_trash_view = self.is_trash_view;
        tab.entries = self.entries.clone();
        tab.trash_entries = self.trash_entries.clone();
        tab.selected = self.selected.clone();
        tab.selected_paths = self.selected_paths.clone();
        tab.selection_anchor = self.selection_anchor.clone();
        tab.expanded_directories = self.expanded_directories.clone();
        tab.back_stack = self.back_stack.clone();
        tab.forward_stack = self.forward_stack.clone();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BrowserTab {
    pub(crate) id: usize,
    pub(crate) directory: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) selected_paths: HashSet<PathBuf>,
    pub(crate) selection_anchor: Option<PathBuf>,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) back_stack: Vec<PathBuf>,
    pub(crate) forward_stack: Vec<PathBuf>,
}

impl BrowserTab {
    pub(crate) fn directory(id: usize, directory: PathBuf) -> Self {
        Self {
            id,
            directory,
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            expanded_directories: HashMap::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        }
    }

    pub(crate) fn trash(id: usize) -> Self {
        Self {
            id,
            directory: trash_location_path(),
            is_trash_view: true,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            expanded_directories: HashMap::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabDragMode {
    Reorder,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabSplitTarget {
    pub(crate) region: SplitRegion,
}

#[derive(Debug, Clone)]
pub(crate) struct TabDragState {
    pub(crate) source_pane_id: BrowserPaneId,
    pub(crate) tab_id: usize,
    pub(crate) phase: FileDragPhase,
    pub(crate) mode: TabDragMode,
    pub(crate) split_target: Option<TabSplitTarget>,
}

impl TabDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneDropTarget {
    Split(SplitRegion),
    Merge(BrowserPaneId),
}

#[derive(Debug, Clone)]
pub(crate) struct PaneDragState {
    pub(crate) source_pane_id: BrowserPaneId,
    pub(crate) phase: FileDragPhase,
    pub(crate) target: Option<PaneDropTarget>,
}

impl PaneDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileDragState {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) target: Option<FileDragTarget>,
    pub(crate) phase: FileDragPhase,
    pub(crate) column_directories_snapshot: Vec<PathBuf>,
}

impl FileDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileDragTarget {
    Directory(PathBuf),
    SidebarBookmarkSlot(SidebarBookmarkDropSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarBookmarkDropSlot {
    Top,
    Bottom,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarBookmarkDragState {
    pub(crate) path: PathBuf,
    pub(crate) origin: Point,
    pub(crate) source_index: usize,
    pub(crate) phase: FileDragPhase,
    pub(crate) order_changed: bool,
}

impl SidebarBookmarkDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDragPhase {
    WaitingForMovement { origin: Point },
    Dragging,
}

#[derive(Debug, Clone)]
pub(crate) struct LastClick {
    pub(crate) path: PathBuf,
    pub(crate) at: Instant,
}

#[derive(Debug, Clone)]
pub(crate) enum PreviewState {
    Loading(PathBuf),
    Ready(PreviewContent),
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) enum PreviewContent {
    Directory {
        entries: Vec<PreviewTreeEntry>,
    },
    Text {
        path: PathBuf,
        rendered: String,
        format: TextPreviewFormat,
    },
    Archive {
        entries: Vec<PreviewTreeEntry>,
    },
    Image {
        path: PathBuf,
        handle: image::Handle,
        width: u32,
        height: u32,
        max_edge: u32,
    },
    Audio {
        path: PathBuf,
        duration: Option<Duration>,
        len: u64,
    },
    Video {
        path: PathBuf,
        frame: Option<image::Handle>,
        width: u32,
        height: u32,
        duration: Option<Duration>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextPreviewFormat {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkdownPreviewMode {
    Rendered,
    Raw,
}

pub(crate) struct TextPreviewDocument {
    path: PathBuf,
    content: text_editor::Content,
    markdown_preview_mode: MarkdownPreviewMode,
}

impl TextPreviewDocument {
    pub(crate) fn new(path: PathBuf, content: &str, format: TextPreviewFormat) -> Self {
        Self {
            path,
            content: text_editor::Content::with_text(&numbered_preview_text(content)),
            markdown_preview_mode: initial_markdown_preview_mode(format),
        }
    }

    pub(crate) fn path(&self) -> &std::path::Path {
        self.path.as_path()
    }

    pub(crate) fn content(&self) -> &text_editor::Content {
        &self.content
    }

    pub(crate) fn markdown_preview_mode(&self) -> MarkdownPreviewMode {
        self.markdown_preview_mode
    }

    pub(crate) fn select_markdown_preview_mode(&mut self, mode: MarkdownPreviewMode) {
        self.markdown_preview_mode = mode;
    }

    pub(crate) fn perform(&mut self, action: text_editor::Action) {
        if action.is_edit() {
            return;
        }

        self.content.perform(action);
    }
}

fn initial_markdown_preview_mode(format: TextPreviewFormat) -> MarkdownPreviewMode {
    match format {
        TextPreviewFormat::Plain => MarkdownPreviewMode::Raw,
        TextPreviewFormat::Markdown => MarkdownPreviewMode::Rendered,
    }
}

pub(crate) fn numbered_preview_text(content: &str) -> String {
    if content.is_empty() {
        return "1 | (empty file)".to_owned();
    }

    let line_count = content.lines().count().max(1);
    let width = line_count.to_string().len();
    let mut numbered = String::new();
    for (index, line) in content.lines().enumerate() {
        numbered.push_str(&format!(
            "{:>width$} | {}\n",
            index + 1,
            line,
            width = width
        ));
    }
    numbered
}

#[derive(Debug, Clone)]
pub(crate) struct AudioPreviewPlayback {
    pub(crate) path: PathBuf,
    pub(crate) runtime: Option<AudioPreviewRuntime>,
    pub(crate) status: AudioPreviewPlaybackStatus,
    pub(crate) position: Duration,
    pub(crate) volume: f32,
    pub(crate) error: Option<String>,
}

impl AudioPreviewPlayback {
    pub(crate) fn loading(path: PathBuf) -> Self {
        Self {
            path,
            runtime: None,
            status: AudioPreviewPlaybackStatus::Loading,
            position: Duration::ZERO,
            volume: 1.0,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPreviewPlaybackStatus {
    Loading,
    Playing,
    Paused,
    Stopped,
    Finished,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPreviewPlayback {
    pub(crate) path: PathBuf,
    pub(crate) audio_runtime: Option<AudioPreviewRuntime>,
    pub(crate) status: VideoPreviewPlaybackStatus,
    pub(crate) position: Duration,
    // Subscription 的身份必须固定；播放进度 tick 只更新 position，不能重建 ffmpeg 流。
    pub(crate) stream_start_position: Duration,
    pub(crate) duration: Option<Duration>,
    pub(crate) volume: f32,
    pub(crate) generation: u64,
    pub(crate) seek_completion: Option<VideoPreviewSeekCompletion>,
    pub(crate) seek_frame_in_flight: Option<Duration>,
    pub(crate) pending_seek_frame: Option<Duration>,
    pub(crate) started_at: Option<Instant>,
    pub(crate) error: Option<String>,
}

impl VideoPreviewPlayback {
    pub(crate) fn playing(path: PathBuf, duration: Option<Duration>) -> Self {
        Self {
            path,
            audio_runtime: None,
            status: VideoPreviewPlaybackStatus::Playing,
            position: Duration::ZERO,
            stream_start_position: Duration::ZERO,
            duration,
            volume: 1.0,
            generation: 1,
            seek_completion: None,
            seek_frame_in_flight: None,
            pending_seek_frame: None,
            started_at: Some(Instant::now()),
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoPreviewSeekCompletion {
    ResumePlayback,
    StayPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoPreviewPlaybackStatus {
    Playing,
    Paused,
    Finished,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct VideoPreviewFrame {
    pub(crate) path: PathBuf,
    pub(crate) generation: u64,
    pub(crate) position: Duration,
    pub(crate) handle: image::Handle,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewWindowProfile {
    Regular,
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone)]
pub(crate) struct PreviewTreeEntry {
    pub(crate) id: usize,
    pub(crate) name: String,
    pub(crate) kind: FileKind,
    pub(crate) depth: usize,
    pub(crate) parent: Option<usize>,
    pub(crate) filesystem_path: Option<PathBuf>,
    pub(crate) directory_children: Option<PreviewTreeDirectoryChildren>,
    pub(crate) is_expanded: bool,
    pub(crate) toggle_rotation_progress: f32,
}

impl PreviewTreeEntry {
    pub(crate) fn from_directory_entry(
        id: usize,
        entry: DirectoryEntry,
        depth: usize,
        parent: Option<usize>,
    ) -> Self {
        let kind = entry.kind;
        Self {
            id,
            name: entry.name().to_string_lossy().into_owned(),
            kind,
            depth,
            parent,
            filesystem_path: Some(entry.path),
            directory_children: preview_tree_directory_children(kind),
            is_expanded: false,
            toggle_rotation_progress: 0.0,
        }
    }

    pub(crate) fn is_directory(&self) -> bool {
        self.kind == FileKind::Directory
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewTreeDirectoryChildren {
    Pending,
    Loading,
    Loaded,
    Error(String),
}

fn preview_tree_directory_children(kind: FileKind) -> Option<PreviewTreeDirectoryChildren> {
    (kind == FileKind::Directory).then_some(PreviewTreeDirectoryChildren::Pending)
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuState {
    FileArea(FileContextMenuState),
    SidebarBookmark(SidebarBookmarkContextMenuState),
}

impl ContextMenuState {
    pub(crate) fn position(&self) -> Point {
        match self {
            Self::FileArea(menu) => menu.position,
            Self::SidebarBookmark(menu) => menu.position,
        }
    }

    pub(crate) fn paste_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::FileArea(menu) => Some(&menu.paste_directory),
            Self::SidebarBookmark(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileContextMenuState {
    pub(crate) target: Option<PathBuf>,
    pub(crate) target_is_directory: bool,
    pub(crate) paste_directory: PathBuf,
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarBookmarkContextMenuState {
    pub(crate) path: PathBuf,
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct ColumnEntryBounds {
    pub(crate) pane_id: BrowserPaneId,
    pub(crate) path: PathBuf,
    pub(crate) bounds: Rectangle,
}

#[derive(Debug, Clone)]
pub(crate) struct SelectionMarquee {
    pub(crate) start: Point,
    pub(crate) current: Point,
    pub(crate) source: SelectionMarqueeSource,
    pub(crate) phase: SelectionMarqueePhase,
    pub(crate) base_selection: HashSet<PathBuf>,
    pub(crate) preserve_existing: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum SelectionMarqueeSource {
    PaneBlank,
    ColumnBlank { directory: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionMarqueePhase {
    WaitingForMovement,
    Selecting,
}

impl SelectionMarquee {
    pub(crate) fn top_left(&self) -> Point {
        Point::new(
            self.start.x.min(self.current.x),
            self.start.y.min(self.current.y),
        )
    }

    pub(crate) fn width(&self) -> f32 {
        (self.current.x - self.start.x).abs().max(1.0)
    }

    pub(crate) fn height(&self) -> f32 {
        (self.current.y - self.start.y).abs().max(1.0)
    }

    pub(crate) fn rectangle(&self) -> Rectangle {
        let top_left = self.top_left();
        Rectangle {
            x: top_left.x,
            y: top_left.y,
            width: self.width(),
            height: self.height(),
        }
    }

    pub(crate) fn is_selecting(&self) -> bool {
        self.phase == SelectionMarqueePhase::Selecting
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarLocation {
    pub(crate) label: String,
    pub(crate) path: PathBuf,
    pub(crate) kind: SidebarLocationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarLocationKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    Bookmark,
}

impl SidebarLocationKind {
    pub(crate) fn is_user_favorite(self) -> bool {
        !matches!(self, Self::Home)
    }
}

pub(crate) const TRASH_LOCATION_LABEL: &str = "Trash";

pub(crate) fn trash_location_path() -> PathBuf {
    PathBuf::from("trash:///")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationMode {
    RecordHistory,
    KeepHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSuggestionDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PathSuggestionRequest {
    pub(crate) input: String,
    pub(crate) current_dir: PathBuf,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchScope {
    CurrentDirectory,
    HomeDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchRequest {
    pub(crate) scope: SearchScope,
    pub(crate) root: PathBuf,
    pub(crate) query: String,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct SearchState {
    pub(crate) scope: SearchScope,
    pub(crate) root: PathBuf,
    pub(crate) query: String,
    pub(crate) request_generation: u64,
    pub(crate) matches: Vec<FileSearchMatch>,
    pub(crate) selected_match: Option<usize>,
    pub(crate) is_loading: bool,
    pub(crate) is_indexing: bool,
    pub(crate) skipped_count: usize,
    pub(crate) error: Option<String>,
    pub(crate) index_error: Option<String>,
}

impl SearchState {
    pub(crate) fn request(&self) -> SearchRequest {
        SearchRequest {
            scope: self.scope,
            root: self.root.clone(),
            query: self.query.clone(),
            generation: self.request_generation,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SearchIndexRuntime {
    pub(crate) base_dir: PathBuf,
    pub(crate) indexing_roots: HashSet<PathBuf>,
    pub(crate) errors: HashMap<PathBuf, String>,
}

impl SearchIndexRuntime {
    pub(crate) fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            indexing_roots: HashSet::new(),
            errors: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExpandedDirectory {
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) status: ExpandedDirectoryStatus,
    pub(crate) is_expanded: bool,
    pub(crate) animation_progress: f32,
}

#[derive(Debug, Clone)]
pub(crate) enum ExpandedDirectoryStatus {
    Loading,
    Loaded,
    Error,
}
