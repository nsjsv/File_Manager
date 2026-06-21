use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use desktop_linux::{
    DesktopClipboardContent, OpenWithApplicationList, StorageDevice, StorageDeviceId,
    TerminalEmulator,
};
use file_core::{DirectoryEntry, DirectoryScan, DirectoryScanBatch, TrashRestoreEntry, TrashScan};
pub(crate) use file_index::SearchMode;
use file_index::{
    DirectoryErrorPolicy, FileSearchIndexMode, FileSearchIndexOutcome, FileSearchIndexStatus,
    FileSearchOutcome, IndexProfile, IndexServiceEvent,
};
use file_operation_store::{StoredTask, TaskQueueStore};
use iced::keyboard;
use iced::widget::text_editor;
use iced::{event, mouse, window, Point, Rectangle, Theme};

use crate::animated_image_preview::{AnimatedImageFrame, AnimatedImagePreview};
use crate::app::archive_creation::ArchiveCreationMessage;
use crate::app::archive_extraction::ArchiveExtractionMessage;
use crate::audio_preview::AudioPreviewRuntime;
use crate::config::{RenderingGpuPreference, SearchBackendMode, UserConfig};
use crate::network_connections::{
    NetworkConnectionMessage, SidebarNetworkConnectionContextMenuState,
};
use crate::operation_history::FileOperationOutcome;
use crate::operation_queue::{FileOperationProgressUpdate, QueuedTransfer};
use crate::shortcuts::ShortcutBindingId;
use crate::sidebar_devices::{SidebarDeviceAction, SidebarDeviceContextMenuState};
use crate::startup_rendering::StartupRenderingEnvironmentStatus;
use crate::thumbnail_cache::ThumbnailLoadOutcome;
use file_core::FileOperationVerification;

pub(crate) use crate::startup_index_tree::{
    StartupIndexDirectoryChildren, StartupIndexEntrySelection, StartupIndexRootSeed,
    StartupIndexSetupState, StartupIndexTreeEntry,
};
pub(crate) use crate::text_preview::{
    MarkdownPreviewMode, TextPreviewChunk, TextPreviewDocument, TextPreviewFormat,
    TextPreviewLineLimitNotice,
};
pub(crate) use file_core::{TransferConflictItem, TransferConflictMetadata};

mod browser_panes;
pub(crate) use browser_panes::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab, BrowserViewMode,
    DirectoryLoadRequest, DirectoryLoadingPlaceholderEntry, ExpandedDirectory,
    ExpandedDirectoryLoadRequest, ExpandedDirectoryStatus, SplitAxis, SplitRegion,
};
mod properties;
pub(crate) use properties::{
    FilePropertiesCategory, FilePropertiesDirectoryContents, FilePropertiesDirectoryContentsState,
    FilePropertiesLoadState, FilePropertiesPermissionAccess, FilePropertiesPermissionClass,
    FilePropertiesPermissionUpdate, FilePropertiesPermissions, FilePropertiesRequest,
    FilePropertiesSnapshot, FilePropertiesState,
};
mod preview;
pub(crate) use preview::{
    AudioPreviewPlayback, AudioPreviewPlaybackStatus, NetworkPreviewCacheFinished,
    NetworkPreviewCacheMessage, NetworkPreviewCacheProgress, NetworkPreviewDownload,
    PreviewContent, PreviewSize, PreviewState, PreviewTreeDirectoryChildren, PreviewTreeEntry,
    PreviewWindowProfile, VideoPreviewFrame, VideoPreviewPlayback, VideoPreviewPlaybackStatus,
    VideoPreviewSeekCompletion,
};
mod search;
pub(crate) use search::{
    SearchIndexDaemonStatus, SearchIndexPathRuleEditMode, SearchIndexPathRuleKind,
    SearchIndexPathRuleSelection, SearchIndexRuntime, SearchRequest, SearchScope, SearchState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchModePromptState;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ScrollbarRegion {
    Sidebar,
    PaneList(BrowserPaneId),
    ColumnBrowser(BrowserPaneId),
    Column {
        pane_id: BrowserPaneId,
        directory: PathBuf,
    },
    SearchResults,
    Settings,
    ShortcutSettings,
    Properties,
    StartupIndexSetup,
    OpenWithApplications,
    OperationQueue,
    PreviewDirectory,
    PreviewArchive,
    MarkdownPreview,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedOperationStore {
    pub(crate) task_queue_store: TaskQueueStore,
    pub(crate) column_width_overrides: HashMap<usize, f32>,
    pub(crate) restored_tasks: Vec<StoredTask>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    StartupEnvironmentLoaded(StartupEnvironment),
    SidebarLocationsLoaded(Vec<SidebarLocation>),
    SidebarDevicesLoaded(Result<Vec<StorageDevice>, String>),
    SidebarDevicesRefreshRequested,
    SidebarDeviceHovered(StorageDeviceId),
    SidebarDeviceHoverCleared(StorageDeviceId),
    SidebarDevicePressed(StorageDeviceId),
    SidebarDeviceMiddlePressed(BrowserPaneId, StorageDeviceId),
    SidebarDeviceRightClicked(StorageDeviceId),
    SidebarDeviceActionSelected(StorageDeviceId, SidebarDeviceAction),
    SidebarDeviceActionFinished(
        StorageDeviceId,
        SidebarDeviceAction,
        Result<Option<PathBuf>, String>,
    ),
    NetworkConnection(NetworkConnectionMessage),
    OperationStoreLoaded(Result<LoadedOperationStore, String>),
    DirectoryLoadBatch(DirectoryLoadRequest, DirectoryScanBatch),
    Loaded(DirectoryLoadRequest, Result<DirectoryScan, String>),
    TrashLoaded(BrowserPaneId, Result<TrashScan, String>),
    OpenFileFinished(PathBuf, Result<(), String>),
    OpenWithRequested(PathBuf),
    OpenWithApplicationsLoaded(PathBuf, Result<OpenWithApplicationList, String>),
    OpenWithDefaultApplicationToggled(bool),
    OpenWithApplicationSelected(String),
    OpenWithApplicationFinished(Result<(), String>),
    OpenTerminalFinished(Result<(), String>),
    PreviewLoaded(PathBuf, Result<PreviewContent, String>),
    NetworkPreviewCache(NetworkPreviewCacheMessage),
    AnimatedImagePreviewLoaded(PathBuf, u64, Result<AnimatedImagePreview, String>),
    FilePropertiesLoaded(
        FilePropertiesRequest,
        Result<FilePropertiesSnapshot, String>,
    ),
    FilePropertiesDirectoryContentsUpdated(FilePropertiesRequest, FilePropertiesDirectoryContents),
    FilePropertiesDirectoryContentsLoaded(
        FilePropertiesRequest,
        Result<FilePropertiesDirectoryContents, String>,
    ),
    FilePropertiesPermissionToggled(
        FilePropertiesPermissionClass,
        FilePropertiesPermissionAccess,
    ),
    FilePropertiesApplyPermissionsToEnclosedItems,
    FilePropertiesCategorySelected(FilePropertiesCategory),
    FilePropertiesPermissionsUpdated(
        FilePropertiesRequest,
        Result<FilePropertiesPermissions, String>,
    ),
    FilePropertiesEnclosedPermissionsUpdated(
        FilePropertiesRequest,
        Result<FilePropertiesPermissions, String>,
    ),
    PreviewDirectoryChildrenLoaded(PathBuf, Result<Vec<DirectoryEntry>, String>),
    TextPreviewAction {
        action: text_editor::Action,
        viewport_height: f32,
    },
    TextPreviewChunkLoaded {
        path: PathBuf,
        generation: u64,
        start_offset: u64,
        outcome: Result<TextPreviewChunk, String>,
    },
    MarkdownPreviewScrolled {
        offset_y: f32,
        viewport_height: f32,
        content_height: f32,
    },
    MarkdownPreviewModeSelected(MarkdownPreviewMode),
    ImagePreviewDimensionsLoaded(PathBuf, Result<(u32, u32), String>),
    AnimatedImageFrameLoaded(AnimatedImageFrame),
    AnimatedImagePreviewFinished(PathBuf, u64),
    AnimatedImagePreviewFailed(PathBuf, u64, String),
    AnimatedImageSeekRequested(f32),
    AnimatedImageSeekCommitted,
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
    BrowserViewModeSelected(BrowserPaneId, BrowserViewMode),
    ListDirectoryToggled(BrowserPaneId, PathBuf),
    ListEntryClicked(BrowserPaneId, PathBuf),
    ColumnEntryClicked(BrowserPaneId, PathBuf),
    ColumnBlankClicked(BrowserPaneId, PathBuf),
    ColumnPlaceholderPressed(BrowserPaneId),
    EntryReleased(BrowserPaneId, PathBuf),
    EntryRightClicked(BrowserPaneId, PathBuf),
    EntryHovered(BrowserPaneId, PathBuf),
    EntryHoverCleared(BrowserPaneId, PathBuf),
    DropTargetHovered(BrowserPaneId, PathBuf),
    DropTargetHoverCleared(BrowserPaneId, PathBuf),
    DropTargetReleased(BrowserPaneId, PathBuf),
    BlankAreaPressed(BrowserPaneId),
    BlankAreaRightClicked(BrowserPaneId, PathBuf),
    SidebarHovered(PathBuf),
    SidebarHoverCleared(PathBuf),
    SidebarPointerMoved(Point),
    SidebarPointerExited,
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
    KeyboardKeyPressed {
        key: keyboard::Key,
        modifiers: keyboard::Modifiers,
        status: event::Status,
    },
    ShortcutCaptureStarted(ShortcutBindingId),
    ShortcutCaptureCanceled,
    ShortcutBindingReset(ShortcutBindingId),
    DragSelectionFinished,
    DismissFloating,
    ArchiveCreation(ArchiveCreationMessage),
    ArchiveExtraction(ArchiveExtractionMessage),
    FileContextMenuExpansionChanged(FileContextMenuExpansion),
    DestructiveActionConfirmed,
    DestructiveActionCanceled,
    AuxiliaryWindowCloseRequested(window::Id),
    AuxiliaryWindowResized(window::Id, f32, f32),
    WindowFocused(window::Id),
    WindowUnfocused(window::Id),
    WindowPointerPressed {
        window: window::Id,
        button: mouse::Button,
        status: event::Status,
    },
    PathInputChanged(BrowserPaneId, String),
    PathInputSubmitted(BrowserPaneId),
    PathSuggestionSelected(BrowserPaneId, PathBuf),
    PathInputStabilized(BrowserPaneId, PathSuggestionRequest),
    PathSuggestionsLoaded(BrowserPaneId, PathSuggestionRequest, Vec<PathBuf>),
    SystemThemeDetected(Theme),
    UserConfigSaved(Result<(), String>),
    ColumnWidthOverrideSaved(Result<(), String>),
    SidebarBookmarksSaved(Result<(), String>),
    SearchInputChanged(String),
    SearchModeSelected(SearchMode),
    SearchInputStabilized(SearchRequest),
    SearchFocusRequested,
    SearchMatchesLoaded(SearchRequest, Result<FileSearchOutcome, String>),
    SearchIndexBuilt(PathBuf, Result<FileSearchIndexOutcome, String>),
    SearchIndexStatusLoaded(PathBuf, Result<FileSearchIndexStatus, String>),
    SearchIndexProfileLoaded(Result<Option<IndexProfile>, String>),
    SearchIndexProfileSaved(Result<IndexProfile, String>),
    SearchIndexProfileDeleted(Result<String, String>),
    SearchIndexDaemonStatusLoaded(Result<SearchIndexDaemonStatus, String>),
    SearchIndexDaemonRestartRequested,
    SearchIndexDaemonRestarted(Result<SearchIndexDaemonStatus, String>),
    SearchIndexMaintenanceEvent(u64, IndexServiceEvent),
    SearchIndexMaintenanceUpdated(u64, Result<bool, String>),
    SearchIndexStatusRefreshRequested,
    SearchIndexManualBuildRequested(PathBuf, FileSearchIndexMode),
    SearchIndexRemoveRequested(PathBuf),
    SearchIndexProfileDeleteRequested,
    SearchIndexMaintenancePauseToggled,
    SearchIndexFailuresClearRequested(PathBuf),
    SearchIndexPathRuleSelected(SearchIndexPathRuleSelection),
    SearchIndexPathRuleKindChanged(SearchIndexPathRuleSelection, SearchIndexPathRuleKind),
    SearchIndexPathRuleKindSelected(SearchIndexPathRuleKind),
    SearchIndexPathRuleInputChanged(String),
    SearchIndexPathRuleEditorCommitted,
    SearchIndexPathRuleAdded,
    SearchIndexPathRuleRemoved,
    SearchIndexPathRuleUpdated,
    SearchIndexDirectoryErrorPolicySelected(DirectoryErrorPolicy),
    SearchIndexContentEnabledToggled(bool),
    SearchIndexMediaEnabledToggled(bool),
    SearchBackendModeSelected(SearchBackendMode),
    SearchModePromptSimpleSelected,
    SearchModePromptIndexedSelected,
    SearchMatchSelected(PathBuf),
    SearchActivated,
    StartupIndexHiddenContentVisibilityToggled,
    StartupIndexEntryToggled(usize),
    StartupIndexDirectoryToggled(usize),
    StartupIndexTreeAnimationTick,
    StartupIndexDirectoryChildrenLoaded(u64, PathBuf, Result<Vec<DirectoryEntry>, String>),
    StartupIndexAccepted,
    StartupIndexSkipped,
    ExpandedDirectoryLoadBatch(ExpandedDirectoryLoadRequest, DirectoryScanBatch),
    ExpandedDirectoryLoaded(ExpandedDirectoryLoadRequest, Result<DirectoryScan, String>),
    ObservedDirectoryChanged(PathBuf),
    SettingsOpened,
    SettingsCategorySelected(SettingsCategory),
    ShowHiddenFilesToggled,
    FileOperationVerificationSelected(FileOperationVerification),
    TerminalEmulatorSelected(TerminalEmulator),
    RenderingGpuPreferenceSelected(RenderingGpuPreference),
    RendererRestartRequested,
    RendererRestartNoticeDismissed,
    CapturedWheelScrolled(mouse::ScrollDelta),
    ScrollbarAutoHideElapsed(ScrollbarRegion, u64),
    WindowChromeAnimationTick,
    SidebarScrolled,
    SearchResultsScrolled,
    SettingsScrolled,
    ShortcutSettingsScrolled,
    PropertiesScrolled,
    StartupIndexSetupScrolled,
    OpenWithApplicationsScrolled,
    OperationQueueScrolled,
    PreviewDirectoryScrolled,
    PreviewArchiveScrolled,
    ColumnScrolled(BrowserPaneId, PathBuf, f32, f32),
    ListScrolled(BrowserPaneId, f32, f32),
    ColumnResizeStarted(BrowserPaneId, usize),
    OpenDirectoryFromMiddleClick(BrowserPaneId, PathBuf),
    OpenTrashInNewTab(BrowserPaneId),
    TabPressed(BrowserPaneId, usize),
    TabCloseRequested(BrowserPaneId, usize),
    TabDragEntered(BrowserPaneId, usize),
    TabDragFinished,
    PaneBack(BrowserPaneId),
    PaneForward(BrowserPaneId),
    PaneUp(BrowserPaneId),
    NavigateTo(PathBuf),
    OpenPath(PathBuf),
    TrashOpened,
    Back,
    Forward,
    RenameInputFocusChecked(bool),
    RenameInputChanged(String),
    BeginRename(PathBuf),
    FilePropertiesRequested(PathBuf),
    OpenTerminalHere(PathBuf),
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
    DeletePermanently { paths: Vec<PathBuf> },
    EmptyTrash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperationQueuePanelMode {
    PassivePreview,
    InteractiveList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    General,
    SearchIndex,
    FileOperations,
    Rendering,
    Shortcuts,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 5] = [
        Self::General,
        Self::SearchIndex,
        Self::FileOperations,
        Self::Rendering,
        Self::Shortcuts,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::SearchIndex => "Search Index",
            Self::FileOperations => "File Operations",
            Self::Rendering => "Rendering",
            Self::Shortcuts => "Shortcuts",
        }
    }
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
    pub(crate) rendering_environment_status: StartupRenderingEnvironmentStatus,
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
    pub(crate) pressed_path: PathBuf,
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
    Insert { index: usize },
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
pub(crate) struct LastActivationClick {
    pub(crate) path: PathBuf,
    pub(crate) at: Instant,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuState {
    FileArea(FileContextMenuState),
    SidebarBookmark(SidebarBookmarkContextMenuState),
    SidebarDevice(SidebarDeviceContextMenuState),
    NetworkConnection(SidebarNetworkConnectionContextMenuState),
}

impl ContextMenuState {
    pub(crate) fn position(&self) -> Point {
        match self {
            Self::FileArea(menu) => menu.position,
            Self::SidebarBookmark(menu) => menu.position,
            Self::SidebarDevice(menu) => menu.position,
            Self::NetworkConnection(menu) => menu.position,
        }
    }

    pub(crate) fn paste_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::FileArea(menu) => Some(&menu.paste_directory),
            Self::SidebarBookmark(_) => None,
            Self::SidebarDevice(_) => None,
            Self::NetworkConnection(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FileContextMenuState {
    pub(crate) target: Option<PathBuf>,
    pub(crate) target_is_directory: bool,
    pub(crate) paste_directory: PathBuf,
    pub(crate) delete_action: FileDeleteAction,
    pub(crate) position: Point,
    pub(crate) expansion: FileContextMenuExpansion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileDeleteAction {
    MoveToTrash,
    DeletePermanently,
    MixedSelection,
}

impl FileDeleteAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::MoveToTrash => "Move to Trash",
            Self::DeletePermanently => "Delete Permanently",
            Self::MixedSelection => "Delete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileContextMenuExpansion {
    None,
    NewEntry,
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
        matches!(
            self,
            Self::Desktop
                | Self::Documents
                | Self::Downloads
                | Self::Pictures
                | Self::Music
                | Self::Videos
                | Self::Bookmark
        )
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
