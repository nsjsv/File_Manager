use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use desktop_linux::{
    DesktopClipboardContent, FileClipboardOperation, OpenWithApplicationList, StorageDevice,
    StorageDeviceId, TerminalEmulator, WaylandDndFileDrop, WaylandDndWindowHandle,
};
use file_core::{DirectoryEntry, DirectoryScan, DirectoryScanBatch, TrashRestoreEntry, TrashScan};
use file_operation_store::{StoredTask, TaskQueueStore};
use iced::keyboard;
use iced::widget::text_editor;
use iced::{event, mouse, window, Point, Rectangle, Theme};

use crate::animated_image_preview::{AnimatedImageFrame, AnimatedImagePreview};
use crate::app::archive_creation::ArchiveCreationMessage;
use crate::app::archive_extraction::ArchiveExtractionMessage;
use crate::audio_preview::AudioPreviewRuntime;
use crate::config::{RenderingGpuPreference, UiLanguage, UiLanguageSetting, UserConfig};
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

pub(crate) use crate::text_preview::{
    MarkdownPreviewMode, TextPreviewChunk, TextPreviewDocument, TextPreviewFormat,
    TextPreviewLineLimitNotice,
};
pub(crate) use file_core::{TransferConflictItem, TransferConflictMetadata};

mod browser_panes;
pub(crate) use browser_panes::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab, BrowserViewMode,
    ColumnBrowserViewport, DirectoryLoadRequest, DirectoryLoadingPlaceholderEntry,
    ExpandedDirectory, ExpandedDirectoryLoadRequest, ExpandedDirectoryStatus, SplitAxis,
    SplitRegion,
};
mod list_view_preferences;
pub(crate) use list_view_preferences::{
    list_column_kind_config_value, list_column_kind_from_config_value, ListColumnConfig,
    ListColumnKind, ListSortPreference, ListViewPreferences,
};
mod list_directory_summary;
pub(crate) use list_directory_summary::{
    ListDirectorySizeDisplayMode, ListDirectorySummary, ListDirectorySummaryCache,
    ListDirectorySummaryLoadRequest,
};
mod batch_rename;
pub(crate) use batch_rename::{
    same_parent, BatchRenameCaseRule, BatchRenameExtensionMode, BatchRenameInsertMode,
    BatchRenameMessage, BatchRenamePreviewRow, BatchRenameRandomMode, BatchRenameRemoveClass,
    BatchRenameRemoveMode, BatchRenameReplaceScope, BatchRenameRulePanel, BatchRenameSliceMode,
    BatchRenameSortMode, BatchRenameSource, BatchRenameState,
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
mod settings;
pub(crate) use settings::SettingsCategory;
mod session;
pub(crate) use session::{
    pane_session_from_live, snapshot_from_stored, snapshot_to_stored, BrowserPaneSession,
    BrowserSessionSnapshot, BrowserTabSession,
};
mod drag;
pub(crate) use drag::{
    FileDragNativeDndState, FileDragPhase, FileDragState, FileDragTarget, LastActivationClick,
    PaneDragPointerPress, PaneDragState, PaneDropTarget, SidebarBookmarkDragState,
    SidebarBookmarkDropSlot, TabDragMode, TabDragState, TabSplitTarget,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ScrollbarRegion {
    Sidebar,
    PaneList(BrowserPaneId),
    ColumnBrowser(BrowserPaneId),
    Column {
        pane_id: BrowserPaneId,
        directory: PathBuf,
    },
    Settings,
    ShortcutSettings,
    Properties,
    OpenWithApplications,
    OperationQueue,
    BatchRenamePreview,
    PreviewDirectory,
    PreviewArchive,
    MarkdownPreview,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedOperationStore {
    pub(crate) task_queue_store: TaskQueueStore,
    pub(crate) column_width_overrides: HashMap<usize, f32>,
    pub(crate) browser_session: Option<BrowserSessionSnapshot>,
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
    ListHeaderRightClicked(BrowserPaneId),
    ListColumnVisibilityToggled(ListColumnKind),
    ListColumnResizeStarted(BrowserPaneId, ListColumnKind),
    ListColumnReorderStarted(BrowserPaneId, ListColumnKind),
    ListColumnReorderTargetEntered(ListColumnKind),
    ListColumnReorderTargetExited(ListColumnKind),
    ListDirectorySummaryLoaded(
        ListDirectorySummaryLoadRequest,
        Result<ListDirectorySummary, String>,
    ),
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
    CursorMoved {
        window: window::Id,
        position: Point,
    },
    CursorLeft(window::Id),
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
    BatchRename(BatchRenameMessage),
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
    UserPreferencesSaved(Result<(), String>),
    AppConfigSaved(Result<(), String>),
    ColumnWidthOverrideSaved(Result<(), String>),
    ExpandedDirectoryLoadBatch(ExpandedDirectoryLoadRequest, DirectoryScanBatch),
    ExpandedDirectoryLoaded(ExpandedDirectoryLoadRequest, Result<DirectoryScan, String>),
    ObservedDirectoryChanged(PathBuf),
    SettingsOpened,
    SettingsCategorySelected(SettingsCategory),
    ShowHiddenFilesToggled,
    ListDirectorySizeDisplayModeToggled,
    NetworkListThumbnailDownloadsToggled,
    MaxPreviewFileMibInputChanged(String),
    MaxPreviewFileMibInputCommitted,
    LanguageSettingSelected(UiLanguageSetting),
    StartupLocationPolicySelected(crate::config::StartupLocationPolicy),
    StartupCustomDirectoryInputChanged(String),
    StartupCustomDirectoryCommitted,
    BrowserSessionSaved(Result<(), String>),
    BrowserSessionSaveDelayElapsed,
    FileOperationVerificationSelected(FileOperationVerification),
    TerminalEmulatorSelected(TerminalEmulator),
    RenderingGpuPreferenceSelected(RenderingGpuPreference),
    RendererRestartRequested,
    RendererRestartNoticeDismissed,
    SmoothScrollWheel(ScrollbarRegion, mouse::ScrollDelta),
    ScrollbarAutoHideElapsed(u64),
    WindowChromeAnimationTick,
    SidebarScrolled,
    SettingsScrolled,
    ShortcutSettingsScrolled,
    PropertiesScrolled,
    OpenWithApplicationsScrolled,
    OperationQueueScrolled,
    BatchRenamePreviewScrolled,
    PreviewDirectoryScrolled,
    PreviewArchiveScrolled,
    ColumnBrowserScrolled(BrowserPaneId, f32, f32),
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
    WaylandDndWindowHandleLoaded(Result<Option<WaylandDndWindowHandle>, String>),
    WaylandFilesDropped(Result<WaylandDndFileDrop, String>),
    FileDropOperationSelected(FileClipboardOperation),
    FileDropCancelled,
    TransferConflictsChecked {
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
        conflicts: Vec<TransferConflictItem>,
    },
    TransferConflictChoiceSelected(TransferConflictChoice),
    TransferConflictApplyToAllToggled,
    TransferConflictCancelRequested,
    SelectAll,
}

#[derive(Debug, Clone)]
pub(crate) enum PendingOperation {
    Copy(Vec<PathBuf>),
    Move(Vec<PathBuf>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileDropPrompt {
    pub(crate) paste_directory: PathBuf,
    pub(crate) paths: Vec<PathBuf>,
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
    Rename,
}

#[derive(Debug, Clone)]
pub(crate) struct TransferConflictState {
    pub(crate) mode: TransferConflictMode,
    pub(crate) transfers: Vec<QueuedTransfer>,
    pub(crate) conflicts: Vec<TransferConflictItem>,
    pub(crate) current_index: usize,
    pub(crate) apply_to_all: bool,
}

impl TransferConflictState {
    pub(crate) fn current_conflict(&self) -> Option<&TransferConflictItem> {
        self.conflicts.get(self.current_index)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StartupEnvironment {
    pub(crate) home: PathBuf,
    pub(crate) system_language: UiLanguage,
    pub(crate) user_config: UserConfig,
    pub(crate) state_database_path: PathBuf,
    pub(crate) rendering_environment_status: StartupRenderingEnvironmentStatus,
}

#[derive(Debug, Clone)]
pub(crate) enum ContextMenuState {
    FileArea(FileContextMenuState),
    ListColumns(ListColumnMenuState),
    SidebarBookmark(SidebarBookmarkContextMenuState),
    SidebarDevice(SidebarDeviceContextMenuState),
    NetworkConnection(SidebarNetworkConnectionContextMenuState),
}

impl ContextMenuState {
    pub(crate) fn position(&self) -> Point {
        match self {
            Self::FileArea(menu) => menu.position,
            Self::ListColumns(menu) => menu.position,
            Self::SidebarBookmark(menu) => menu.position,
            Self::SidebarDevice(menu) => menu.position,
            Self::NetworkConnection(menu) => menu.position,
        }
    }

    pub(crate) fn paste_directory(&self) -> Option<&PathBuf> {
        match self {
            Self::FileArea(menu) => Some(&menu.paste_directory),
            Self::ListColumns(_) => None,
            Self::SidebarBookmark(_) => None,
            Self::SidebarDevice(_) => None,
            Self::NetworkConnection(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ListColumnMenuState {
    pub(crate) position: Point,
}

#[derive(Debug, Clone)]
pub(crate) struct FileContextMenuState {
    pub(crate) target: Option<PathBuf>,
    pub(crate) target_is_directory: bool,
    pub(crate) paste_directory: PathBuf,
    pub(crate) can_batch_rename: bool,
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
