use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use desktop_linux::DesktopClipboardContent;
use file_core::{
    DirectoryEntry, DirectoryScan, FileKind, FileSearchIndexOutcome, FileSearchMatch,
    FileSearchOutcome, TrashEntry, TrashScan,
};
use file_operation_store::TaskQueueStore;
use iced::keyboard;
use iced::widget::image;
use iced::{event, mouse, window, Point, Theme};

use crate::audio_preview::AudioPreviewRuntime;
use crate::config::UserConfig;
use crate::operation_queue::{FileOperationProgressUpdate, QueuedTransfer};
use crate::thumbnail_cache::ThumbnailLoadOutcome;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    InitialLoadFinished(InitialLoad),
    Loaded(Result<DirectoryScan, String>),
    TrashLoaded(Result<TrashScan, String>),
    OpenFileFinished(Result<(), String>),
    PreviewLoaded(PathBuf, Result<PreviewContent, String>),
    ImagePreviewDimensionsLoaded(PathBuf, Result<(u32, u32), String>),
    AudioPreviewPlaybackToggled,
    AudioPreviewStarted(PathBuf, Result<AudioPreviewRuntime, String>),
    AudioPreviewSeekRequested(f32),
    AudioPreviewVolumeChanged(f32),
    AudioPreviewTick,
    VideoPreviewPlaybackToggled,
    VideoPreviewAudioStarted(PathBuf, u64, Result<AudioPreviewRuntime, String>),
    VideoPreviewSeekRequested(f32),
    VideoPreviewVolumeChanged(f32),
    VideoPreviewTick,
    VideoPreviewFrameLoaded(VideoPreviewFrame),
    VideoPreviewSeekFrameFailed(PathBuf, u64, String),
    VideoPreviewFinished(PathBuf, u64),
    VideoPreviewFailed(PathBuf, u64, String),
    FileOperationProgressed(u64, FileOperationProgressUpdate),
    FileOperationFinished(u64, Result<(), String>),
    FileOperationIndicatorPressed,
    FileOperationAutoHideElapsed(u64),
    FileOperationPauseToggled(u64),
    FileOperationCancelRequested(u64),
    PreviewTreeDirectoryToggled(usize),
    PreviewTreeAnimationTick,
    ThumbnailBatchLoaded(Vec<ThumbnailLoadOutcome>),
    ColumnEntryClicked(PathBuf),
    ColumnBlankClicked(PathBuf),
    EntryReleased,
    EntryRightClicked(PathBuf),
    EntryHovered(PathBuf),
    EntryHoverCleared(PathBuf),
    DropTargetHovered(PathBuf),
    DropTargetHoverCleared(PathBuf),
    BlankAreaPressed,
    BlankAreaRightClicked(PathBuf),
    SidebarHovered(PathBuf),
    SidebarHoverCleared(PathBuf),
    CursorMoved(Point),
    ColumnBrowserCursorEntered,
    ColumnBrowserCursorExited,
    KeyboardModifiersChanged(keyboard::Modifiers),
    DragSelectionFinished,
    DismissFloating,
    AuxiliaryWindowCloseRequested(window::Id),
    AuxiliaryWindowResized(window::Id, u32, u32),
    WindowFocused(window::Id),
    WindowUnfocused(window::Id),
    FocusedWindowEscapePressed,
    WindowPointerPressed {
        button: mouse::Button,
        status: event::Status,
    },
    CapturedPreviewShortcutPressed,
    RequestPreview,
    PathInputChanged(String),
    PathInputSubmitted,
    PathSuggestionSelected(PathBuf),
    PathSuggestionMoved(PathSuggestionDirection),
    PathSuggestionCompleted(PathSuggestionDirection),
    PathSuggestionsLoaded(String, Vec<PathBuf>),
    SystemThemeDetected(Theme),
    UserConfigSaved(Result<(), String>),
    SearchOpened,
    SearchInputChanged(String),
    SearchFocusRequested,
    SearchMatchesLoaded(SearchRequest, Result<FileSearchOutcome, String>),
    SearchIndexBuilt(PathBuf, Result<FileSearchIndexOutcome, String>),
    SearchMatchSelected(PathBuf),
    SearchActivated,
    ExpandedDirectoryLoaded(PathBuf, Result<DirectoryScan, String>),
    ObservedDirectoryChanged(PathBuf),
    ColumnSettingsToggled,
    ShowHiddenFilesToggled,
    ColumnViewModeSelected(ColumnViewMode),
    ColumnFixedCountSelected(usize),
    ColumnBrowserWheelScrolled(mouse::ScrollDelta),
    ColumnScrolled(PathBuf, f32, f32),
    ColumnResizeStarted,
    OpenDirectoryInNewTab(PathBuf),
    OpenTrashInNewTab,
    TabPressed(usize),
    TabCloseRequested(usize),
    TabDragEntered(usize),
    TabDragFinished,
    NavigateTo(PathBuf),
    TrashOpened,
    Up,
    Back,
    Forward,
    RenameInputFocusChecked(bool),
    RenameInputChanged(String),
    BeginRename(PathBuf),
    RenameSelected,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationQueuePanelMode {
    PassivePreview,
    InteractiveList,
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
pub(crate) struct TransferConflictMetadata {
    pub(crate) is_directory: bool,
    pub(crate) len: u64,
    pub(crate) modified: Option<SystemTime>,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferConflictItem {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
    pub(crate) source_metadata: TransferConflictMetadata,
    pub(crate) target_metadata: TransferConflictMetadata,
}

impl TransferConflictItem {
    pub(crate) fn can_merge(&self) -> bool {
        self.source_metadata.is_directory && self.target_metadata.is_directory
    }
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
pub(crate) struct InitialLoad {
    pub(crate) home: PathBuf,
    pub(crate) scan: Result<DirectoryScan, String>,
    pub(crate) sidebar_locations: Vec<SidebarLocation>,
    pub(crate) user_config: UserConfig,
    pub(crate) operation_store: Result<TaskQueueStore, String>,
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

#[derive(Debug, Clone)]
pub(crate) struct FileDragState {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) target_directory: Option<PathBuf>,
    pub(crate) phase: FileDragPhase,
    pub(crate) column_directories_snapshot: Vec<PathBuf>,
}

impl FileDragState {
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.phase, FileDragPhase::Dragging)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FileDragPhase {
    WaitingForMovement { origin: Point },
    Dragging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnViewMode {
    Unbounded,
    Fixed,
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
        path: PathBuf,
        entries: Vec<PreviewTreeEntry>,
        total: usize,
        skipped: usize,
        truncated: bool,
    },
    Text {
        path: PathBuf,
        content: String,
        truncated: bool,
    },
    Archive {
        path: PathBuf,
        entries: Vec<PreviewTreeEntry>,
        total: usize,
        truncated: bool,
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
    pub(crate) duration: Option<Duration>,
    pub(crate) volume: f32,
    pub(crate) generation: u64,
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
            duration,
            volume: 1.0,
            generation: 1,
            started_at: Some(Instant::now()),
            error: None,
        }
    }
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
    pub(crate) is_expanded: bool,
    pub(crate) toggle_rotation_progress: f32,
}

impl PreviewTreeEntry {
    pub(crate) fn is_directory(&self) -> bool {
        self.kind == FileKind::Directory
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMenuState {
    pub(crate) target: Option<PathBuf>,
    pub(crate) target_is_directory: bool,
    pub(crate) paste_directory: PathBuf,
    pub(crate) position: Point,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SelectionMarquee {
    pub(crate) start: Point,
    pub(crate) current: Point,
}

impl SelectionMarquee {
    pub(crate) fn top_left(self) -> Point {
        Point::new(
            self.start.x.min(self.current.x),
            self.start.y.min(self.current.y),
        )
    }

    pub(crate) fn width(self) -> f32 {
        (self.current.x - self.start.x).abs().max(1.0)
    }

    pub(crate) fn height(self) -> f32 {
        (self.current.y - self.start.y).abs().max(1.0)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SidebarLocation {
    pub(crate) label: String,
    pub(crate) path: PathBuf,
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
}

#[derive(Debug, Clone)]
pub(crate) struct SearchState {
    pub(crate) scope: SearchScope,
    pub(crate) root: PathBuf,
    pub(crate) query: String,
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
