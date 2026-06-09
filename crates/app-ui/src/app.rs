mod column_resize;
mod column_scroll;
mod events;
mod file_operations;
mod navigation;
pub(crate) mod panes;
mod paths;
mod preview_state;
mod rendering_settings;
mod runtime;
mod scrollbar;
mod search;
mod selection;
mod sidebar_bookmarks;
mod startup;
mod tabs;
mod text_input_shortcuts;
mod thumbnailing;
mod windows;

pub(crate) use runtime::run;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use desktop_linux::TerminalEmulator;
use file_core::{DirectoryEntry, ScanOptions, TrashEntry};
use iced::event;
use iced::keyboard;
use iced::window;
use iced::{time, Element, Point, Subscription, Task, Theme};

use crate::app::column_resize::ColumnResizeDrag;
use crate::app::events::global_event_message;
use crate::app::runtime::{
    directory_watch_subscription, operation_queue_auto_hide_command, system_theme_command,
};
use crate::app::scrollbar::{ScrollbarAnimation, SCROLLBAR_ANIMATION_INTERVAL};
use crate::app::sidebar_bookmarks::SidebarBookmarkMotionState;
use crate::app::tabs::{TabAnimationState, TabBarReveal};
use crate::app::windows::{
    default_preview_size, main_window_settings, MAIN_WINDOW_INITIAL_HEIGHT,
    MAIN_WINDOW_INITIAL_WIDTH,
};
use crate::commands::{file_operation_subscription, startup_environment_command};
use crate::config;
use crate::model::{
    AudioPreviewPlayback, BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserTab,
    ContextMenuState, DestructiveActionConfirmation, ExpandedDirectory, FileDragState, Message,
    NavigationMode, OperationQueuePanelMode, PendingOperation, PreviewSize, PreviewState,
    PreviewWindowProfile, ScrollbarVisibility, SearchIndexRuntime, SearchState, SelectionMarquee,
    SidebarBookmarkDragState, SidebarBookmarkDropSlot, SidebarLocation, TabDragState,
    TextPreviewDocument, TransferConflictState, VideoPreviewPlayback,
};
use crate::operation_history::FileOperationHistory;
use crate::operation_queue::FileOperationQueue;
use crate::startup_trace;
use crate::thumbnail_cache::{ColumnViewport, ThumbnailCache};
use crate::video_preview::video_preview_subscription;
use crate::view::{rename_input_id, view_browser, view_preview_window, view_search_window};

const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(500);
const POINTER_DRAG_ACTIVATION_DISTANCE: f32 = 3.0;
const PREVIEW_TREE_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const AUDIO_PREVIEW_TICK_INTERVAL: Duration = Duration::from_millis(250);
const COLUMN_BROWSER_WHEEL_LINE_PIXELS: f32 = 60.0;

pub(crate) struct FileBrowser {
    pub(crate) current_dir: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    selected_paths: HashSet<PathBuf>,
    pub(crate) hovered_entry: Option<PathBuf>,
    pub(crate) hovered_sidebar: Option<PathBuf>,
    cursor_paste_directory: Option<PathBuf>,
    cursor_search_directory: Option<PathBuf>,
    pub(crate) preview: Option<PreviewState>,
    pub(crate) text_preview_document: Option<TextPreviewDocument>,
    pub(crate) audio_preview: Option<AudioPreviewPlayback>,
    pub(crate) video_preview: Option<VideoPreviewPlayback>,
    pub(crate) preview_size: PreviewSize,
    pending_preview_resize: Option<PreviewSize>,
    preview_window_profile: PreviewWindowProfile,
    main_window: window::Id,
    preview_window: Option<window::Id>,
    focused_window: window::Id,
    pub(crate) thumbnail_cache: ThumbnailCache,
    pub(crate) column_viewports: HashMap<PathBuf, ColumnViewport>,
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) sidebar_locations: Vec<SidebarLocation>,
    pub(crate) sidebar_bookmark_drop_slot: Option<SidebarBookmarkDropSlot>,
    pub(crate) sidebar_bookmark_drag: Option<SidebarBookmarkDragState>,
    pub(crate) sidebar_bookmark_motion: HashMap<PathBuf, SidebarBookmarkMotionState>,
    pub(crate) renaming: Option<PathBuf>,
    pending_created_entry_rename: Option<PathBuf>,
    pub(crate) pending_operation: Option<PendingOperation>,
    pub(crate) transfer_conflict: Option<TransferConflictState>,
    pub(crate) destructive_action_confirmation: Option<DestructiveActionConfirmation>,
    pub(crate) search: Option<SearchState>,
    search_window: Option<window::Id>,
    pub(crate) search_index: SearchIndexRuntime,
    pub(crate) tabs: Vec<BrowserTab>,
    pub(crate) active_tab_id: usize,
    tab_bar_reveal: TabBarReveal,
    pub(crate) tab_animations: HashMap<usize, TabAnimationState>,
    pub(crate) panes: Vec<BrowserPane>,
    pub(crate) pane_layout: BrowserPaneLayout,
    tab_drag: Option<TabDragState>,
    pub(crate) selection_marquee: Option<SelectionMarquee>,
    pub(crate) file_drag: Option<FileDragState>,
    pub(crate) options: ScanOptions,
    user_config: config::UserConfig,
    pub(crate) rendering_gpu_preference: config::RenderingGpuPreference,
    pub(crate) renderer_restart_notice_visible: bool,
    pub(crate) path_input: String,
    pub(crate) path_suggestions: Vec<PathBuf>,
    pub(crate) path_suggestion_selection: Option<usize>,
    path_suggestion_generation: u64,
    pub(crate) column_width_override: Option<f32>,
    column_width_reference_content_width: Option<f32>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) is_column_view_settings_open: bool,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) rename_input: String,
    pub(crate) is_loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) cursor_position: Point,
    pub(crate) main_window_width: f32,
    pub(crate) main_window_height: f32,
    is_cursor_over_column_browser: bool,
    keyboard_modifiers: keyboard::Modifiers,
    selection_anchor: Option<PathBuf>,
    drag_selection_anchor: Option<PathBuf>,
    column_resize_drag: Option<ColumnResizeDrag>,
    last_click: Option<crate::model::LastClick>,
    pub(crate) operation_queue: FileOperationQueue,
    operation_history: FileOperationHistory,
    pub(crate) operation_queue_panel_mode: OperationQueuePanelMode,
    operation_queue_auto_hide_generation: u64,
    pub(crate) scrollbar_visibility: ScrollbarVisibility,
    scrollbar_auto_hide_generation: u64,
    scrollbar_animation: Option<ScrollbarAnimation>,
    pending_search_reveal: Option<PathBuf>,
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    next_tab_id: usize,
    next_pane_id: u64,
    theme: Theme,
    is_shutting_down: bool,
}

impl FileBrowser {
    fn boot() -> (Self, Task<Message>) {
        let (main_window, open_main_window) = window::open(main_window_settings());
        let user_config = config::ui_thread_startup_config();
        let (browser, initial_tasks) = Self::new_with_main_window(user_config, main_window);

        (
            browser,
            Task::batch([open_main_window.discard(), initial_tasks]),
        )
    }

    #[cfg(test)]
    pub(crate) fn new(user_config: config::UserConfig) -> (Self, Task<Message>) {
        Self::new_with_main_window(user_config, window::Id::unique())
    }

    fn new_with_main_window(
        user_config: config::UserConfig,
        main_window: window::Id,
    ) -> (Self, Task<Message>) {
        startup_trace::mark_once("file_browser_new_started");
        let placeholder_dir = PathBuf::from("/");
        let options = ScanOptions::default();
        let initial_tab = BrowserTab::directory(0, placeholder_dir.clone());
        let initial_pane = BrowserPane {
            id: BrowserPaneId::PRIMARY,
            current_dir: placeholder_dir.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            expanded_directories: HashMap::new(),
            column_viewports: HashMap::new(),
            tabs: vec![initial_tab.clone()],
            active_tab_id: initial_tab.id,
            path_input: String::new(),
            path_suggestions: Vec::new(),
            path_suggestion_selection: None,
            path_suggestion_generation: 0,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            is_loading: true,
        };
        let mut browser = Self {
            current_dir: placeholder_dir.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            hovered_entry: None,
            hovered_sidebar: None,
            cursor_paste_directory: None,
            cursor_search_directory: None,
            preview: None,
            text_preview_document: None,
            audio_preview: None,
            video_preview: None,
            preview_size: default_preview_size(PreviewWindowProfile::Regular),
            pending_preview_resize: None,
            preview_window_profile: PreviewWindowProfile::Regular,
            main_window,
            preview_window: None,
            focused_window: main_window,
            thumbnail_cache: ThumbnailCache::new(user_config.thumbnail_cache_dir.clone()),
            column_viewports: HashMap::new(),
            context_menu: None,
            sidebar_locations: Vec::new(),
            sidebar_bookmark_drop_slot: None,
            sidebar_bookmark_drag: None,
            sidebar_bookmark_motion: HashMap::new(),
            renaming: None,
            pending_created_entry_rename: None,
            pending_operation: None,
            transfer_conflict: None,
            destructive_action_confirmation: None,
            search: None,
            search_window: None,
            search_index: SearchIndexRuntime::new(PathBuf::new()),
            tabs: vec![initial_tab],
            active_tab_id: 0,
            tab_bar_reveal: TabBarReveal::default(),
            tab_animations: HashMap::new(),
            panes: vec![initial_pane],
            pane_layout: BrowserPaneLayout::Single {
                active: BrowserPaneId::PRIMARY,
            },
            tab_drag: None,
            selection_marquee: None,
            file_drag: None,
            options: options.clone(),
            user_config: user_config.clone(),
            rendering_gpu_preference: user_config.rendering_gpu_preference,
            renderer_restart_notice_visible: false,
            path_input: String::new(),
            path_suggestions: Vec::new(),
            path_suggestion_selection: None,
            path_suggestion_generation: 0,
            column_width_override: user_config.legacy_column_width_override,
            column_width_reference_content_width: None,
            terminal_emulator: user_config.terminal_emulator,
            is_column_view_settings_open: false,
            expanded_directories: HashMap::new(),
            rename_input: String::new(),
            is_loading: true,
            error: None,
            cursor_position: Point::new(0.0, 0.0),
            main_window_width: MAIN_WINDOW_INITIAL_WIDTH,
            main_window_height: MAIN_WINDOW_INITIAL_HEIGHT,
            is_cursor_over_column_browser: false,
            keyboard_modifiers: keyboard::Modifiers::default(),
            selection_anchor: None,
            drag_selection_anchor: None,
            column_resize_drag: None,
            last_click: None,
            operation_queue: FileOperationQueue::new(),
            operation_history: FileOperationHistory::new(),
            operation_queue_panel_mode: OperationQueuePanelMode::PassivePreview,
            operation_queue_auto_hide_generation: 0,
            scrollbar_visibility: ScrollbarVisibility::Hidden,
            scrollbar_auto_hide_generation: 0,
            scrollbar_animation: None,
            pending_search_reveal: None,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            next_tab_id: 1,
            next_pane_id: 1,
            theme: Theme::Light,
            is_shutting_down: false,
        };
        browser.refresh_column_width_reference_content_width();
        startup_trace::mark_once("file_browser_new_ready");
        (
            browser,
            Task::batch([startup_environment_command(), system_theme_command()]),
        )
    }

    fn title(&self, window: window::Id) -> String {
        self.window_title(window)
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.is_shutting_down {
            return Subscription::none();
        }

        let mut subscriptions = vec![event::listen_with(global_event_message)];

        if !self.is_loading && !self.is_trash_view {
            subscriptions.push(directory_watch_subscription(self.current_dir.clone()));
            subscriptions.extend(
                self.expanded_directories
                    .keys()
                    .cloned()
                    .map(directory_watch_subscription),
            );
        }

        if let Some(operation) = self.operation_queue.active_subscription() {
            subscriptions.push(file_operation_subscription(operation));
        }

        if self.preview_tree_animation_is_active() {
            subscriptions.push(
                time::every(PREVIEW_TREE_ANIMATION_INTERVAL)
                    .map(|_| Message::PreviewTreeAnimationTick),
            );
        }

        if self.scrollbar_animation_is_active()
            || self.tab_bar_reveal_animation_is_active()
            || self.tab_animation_is_active()
            || self.sidebar_bookmark_motion_is_active()
        {
            subscriptions.push(
                time::every(SCROLLBAR_ANIMATION_INTERVAL)
                    .map(|_| Message::WindowChromeAnimationTick),
            );
        }

        if self.audio_preview_is_active() {
            subscriptions
                .push(time::every(AUDIO_PREVIEW_TICK_INTERVAL).map(|_| Message::AudioPreviewTick));
        }

        if self.video_preview_is_active() {
            subscriptions
                .push(time::every(AUDIO_PREVIEW_TICK_INTERVAL).map(|_| Message::VideoPreviewTick));
        }

        if let Some((path, generation, position)) = self.active_video_preview_stream() {
            subscriptions.push(video_preview_subscription(path, generation, position));
        }

        Subscription::batch(subscriptions)
    }

    fn theme(&self, _window: window::Id) -> Theme {
        self.theme.clone()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::StartupEnvironmentLoaded(startup_environment) => {
                self.accept_startup_environment(startup_environment)
            }
            Message::SidebarLocationsLoaded(sidebar_locations) => {
                self.accept_sidebar_locations(sidebar_locations)
            }
            Message::OperationStoreLoaded(operation_store) => {
                self.accept_operation_store(operation_store)
            }
            Message::Loaded(pane_id, Ok(scan)) => self.accept_directory_scan(pane_id, scan),
            Message::Loaded(pane_id, Err(error)) => {
                if pane_id == self.active_pane_id() {
                    self.is_loading = false;
                } else if let Some(pane) = self.pane_by_id_mut(pane_id) {
                    pane.is_loading = false;
                }
                self.error = Some(error);
                Task::none()
            }
            Message::TrashLoaded(pane_id, Ok(scan)) => self.accept_trash_scan(pane_id, scan),
            Message::TrashLoaded(pane_id, Err(error)) => {
                if pane_id == self.active_pane_id() {
                    self.is_loading = false;
                } else if let Some(pane) = self.pane_by_id_mut(pane_id) {
                    pane.is_loading = false;
                }
                self.error = Some(error);
                Task::none()
            }
            Message::OpenFileFinished(Ok(())) => {
                self.error = None;
                Task::none()
            }
            Message::OpenFileFinished(Err(error)) => {
                self.error = Some(error);
                Task::none()
            }
            Message::OpenTerminalFinished(Ok(())) => {
                self.error = None;
                Task::none()
            }
            Message::OpenTerminalFinished(Err(error)) => {
                self.error = Some(error);
                Task::none()
            }
            Message::PreviewLoaded(path, preview_outcome) => {
                self.accept_preview(path, preview_outcome)
            }
            Message::PreviewDirectoryChildrenLoaded(parent_path, children_outcome) => {
                self.accept_preview_directory_children(parent_path, children_outcome)
            }
            Message::TextPreviewAction(action) => self.handle_text_preview_action(action),
            Message::MarkdownPreviewModeSelected(mode) => {
                if let Some(document) = self.text_preview_document.as_mut() {
                    document.select_markdown_preview_mode(mode);
                }
                Task::none()
            }
            Message::ImagePreviewDimensionsLoaded(path, dimensions_outcome) => {
                self.accept_image_preview_dimensions(path, dimensions_outcome)
            }
            Message::AudioPreviewPlaybackToggled => self.toggle_audio_preview_playback(),
            Message::AudioPreviewStarted(path, playback_outcome) => {
                self.accept_audio_preview_started(path, playback_outcome)
            }
            Message::AudioPreviewSeekRequested(position) => {
                self.seek_audio_preview_playback(position)
            }
            Message::AudioPreviewVolumeChanged(volume) => self.change_audio_preview_volume(volume),
            Message::AudioPreviewTick => self.update_audio_preview_playback(),
            Message::VideoPreviewPlaybackToggled => self.toggle_video_preview_playback(),
            Message::VideoPreviewAudioStarted(path, generation, audio_outcome) => {
                self.accept_video_preview_audio_started(path, generation, audio_outcome)
            }
            Message::VideoPreviewMetadataLoaded(path, metadata_outcome) => {
                self.accept_video_preview_metadata(path, metadata_outcome)
            }
            Message::VideoPreviewSeekRequested(position) => {
                self.seek_video_preview_playback(position)
            }
            Message::VideoPreviewSeekCommitted => self.commit_video_preview_seek(),
            Message::VideoPreviewVolumeChanged(volume) => self.change_video_preview_volume(volume),
            Message::VideoPreviewTick => self.update_video_preview_playback(),
            Message::VideoPreviewFrameLoaded(frame) => self.accept_video_preview_frame(frame),
            Message::VideoPreviewSeekFrameFailed(path, generation, position, error) => {
                self.accept_video_preview_seek_frame_error(path, generation, position, error)
            }
            Message::VideoPreviewFinished(path, generation) => {
                self.accept_video_preview_finished(path, generation)
            }
            Message::VideoPreviewFailed(path, generation, error) => {
                self.accept_video_preview_error(path, generation, error)
            }
            Message::FileOperationProgressed(task_id, progress) => {
                if let Some(error) = self.operation_queue.update_progress(task_id, progress) {
                    self.error = Some(error);
                }
                Task::none()
            }
            Message::FileOperationFinished(task_id, result) => {
                self.accept_file_operation_finished(task_id, result)
            }
            Message::FileOperationIndicatorPressed => {
                self.context_menu = None;
                self.is_column_view_settings_open = false;
                if self.operation_queue.is_panel_open()
                    && self.operation_queue_panel_mode == OperationQueuePanelMode::InteractiveList
                {
                    self.operation_queue.close_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
                } else {
                    self.operation_queue.open_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::InteractiveList;
                }
                self.operation_queue_auto_hide_generation =
                    self.operation_queue_auto_hide_generation.wrapping_add(1);
                Task::none()
            }
            Message::FileOperationAutoHideElapsed(generation) => {
                if generation == self.operation_queue_auto_hide_generation {
                    self.operation_queue.close_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
                }
                Task::none()
            }
            Message::FileOperationPauseToggled(task_id) => {
                if let Some(error) = self.operation_queue.toggle_pause(task_id) {
                    self.error = Some(error);
                }
                Task::none()
            }
            Message::FileOperationCancelRequested(task_id) => {
                if let Some(error) = self.operation_queue.cancel(task_id) {
                    self.error = Some(error);
                }
                Task::none()
            }
            Message::PreviewTreeDirectoryToggled(entry_id) => {
                self.toggle_preview_tree_directory(entry_id)
            }
            Message::PreviewTreeAnimationTick => self.advance_preview_tree_animation(),
            Message::ThumbnailRefreshRequested(pane_id, directory) => {
                self.accept_thumbnail_refresh_request(pane_id, directory)
            }
            Message::ThumbnailBatchLoaded(outcomes) => self.accept_thumbnail_batch(outcomes),
            Message::ColumnEntryClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                self.handle_column_entry_clicked(path)
            }
            Message::ColumnBlankClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                self.handle_column_blank_clicked(path)
            }
            Message::EntryReleased(pane_id) => {
                self.activate_pane(pane_id);
                self.finish_tab_drag();
                Task::batch([
                    self.finish_sidebar_bookmark_drag(),
                    self.finish_column_resize_drag_command(),
                    self.finish_drag_selection(),
                ])
            }
            Message::EntryRightClicked(pane_id, path) => {
                self.activate_pane(pane_id);
                self.handle_entry_right_clicked(path)
            }
            Message::EntryHovered(pane_id, path) => {
                if pane_id != self.active_pane_id() {
                    return Task::none();
                }
                self.cursor_search_directory = Some(
                    path.parent()
                        .map(|parent| parent.to_path_buf())
                        .unwrap_or_else(|| self.current_dir.clone()),
                );
                self.handle_entry_hovered(path)
            }
            Message::EntryHoverCleared(pane_id, path) => {
                if pane_id != self.active_pane_id() {
                    return Task::none();
                }
                self.handle_entry_hover_cleared(path)
            }
            Message::DropTargetHovered(pane_id, directory) => {
                if pane_id != self.active_pane_id() {
                    return Task::none();
                }
                self.cursor_search_directory = Some(directory.clone());
                self.handle_drop_target_hovered(directory)
            }
            Message::DropTargetHoverCleared(pane_id, directory) => {
                if pane_id != self.active_pane_id() {
                    return Task::none();
                }
                if self.cursor_search_directory.as_ref() == Some(&directory) {
                    self.cursor_search_directory = None;
                }
                self.handle_drop_target_hover_cleared(directory)
            }
            Message::BlankAreaPressed(pane_id) => {
                self.activate_pane(pane_id);
                self.start_selection_marquee()
            }
            Message::BlankAreaRightClicked(pane_id, directory) => {
                self.activate_pane(pane_id);
                self.handle_blank_area_right_clicked(directory)
            }
            Message::SidebarHovered(path) => self.handle_sidebar_hovered(path),
            Message::SidebarHoverCleared(path) => self.handle_sidebar_hover_cleared(path),
            Message::SidebarPointerMoved(position) => {
                self.update_sidebar_bookmark_drop_slot(position)
            }
            Message::SidebarPointerExited => self.clear_sidebar_bookmark_drop_slot(),
            Message::SidebarBookmarkDropSlotHovered(slot) => {
                self.handle_sidebar_bookmark_drop_slot_hovered(slot)
            }
            Message::SidebarBookmarkDropSlotCleared(slot) => {
                self.handle_sidebar_bookmark_drop_slot_cleared(slot)
            }
            Message::SidebarBookmarkPressed(path) => self.start_sidebar_bookmark_drag(path),
            Message::SidebarBookmarkRightClicked(path) => {
                self.handle_sidebar_bookmark_right_clicked(path)
            }
            Message::SidebarBookmarkEntered(path) => self.handle_sidebar_bookmark_entered(path),
            Message::SidebarBookmarkReleased => self.finish_sidebar_bookmark_drag(),
            Message::SidebarBookmarkDeleteRequested(path) => self.delete_sidebar_bookmark(path),
            Message::CursorMoved(position) => {
                self.cursor_position = position;
                self.update_tab_drag(position);
                self.update_file_drag(position);
                self.update_sidebar_bookmark_drag(position);
                self.update_column_resize_drag(position);
                self.update_selection_marquee(position);
                Task::none()
            }
            Message::ColumnBrowserCursorEntered(pane_id) => {
                self.activate_pane(pane_id);
                self.is_cursor_over_column_browser = true;
                Task::none()
            }
            Message::ColumnBrowserCursorExited(pane_id) => {
                if pane_id == self.active_pane_id() {
                    self.is_cursor_over_column_browser = false;
                    self.cursor_search_directory = None;
                    self.clear_cursor_paste_target()
                } else {
                    Task::none()
                }
            }
            Message::KeyboardModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
                Task::none()
            }
            Message::DragSelectionFinished => {
                self.finish_tab_drag();
                Task::batch([
                    self.finish_sidebar_bookmark_drag(),
                    self.finish_column_resize_drag_command(),
                    self.finish_drag_selection(),
                ])
            }
            Message::DismissFloating => self.dismiss_floating(),
            Message::DestructiveActionConfirmed => self.confirm_destructive_action(),
            Message::DestructiveActionCanceled => self.cancel_destructive_action(),
            Message::AuxiliaryWindowCloseRequested(window) => self.close_auxiliary_window(window),
            Message::AuxiliaryWindowResized(window, width, height) => {
                self.handle_auxiliary_window_resized(window, width, height)
            }
            Message::WindowFocused(window) => self.handle_window_focused(window),
            Message::WindowUnfocused(window) => self.handle_window_unfocused(window),
            Message::FocusedWindowEscapePressed => self.handle_focused_window_escape_pressed(),
            Message::WindowPointerPressed { button, status } => {
                self.handle_window_pointer_pressed(button, status)
            }
            Message::CapturedPreviewShortcutPressed => self.handle_captured_preview_shortcut(),
            Message::RequestPreview => self.request_preview(),
            Message::PathInputChanged(pane_id, value) => {
                self.activate_pane(pane_id);
                self.update_path_input(value)
            }
            Message::PathInputSubmitted(pane_id) => {
                self.activate_pane(pane_id);
                self.submit_path_input()
            }
            Message::PathSuggestionSelected(pane_id, path) => {
                self.activate_pane(pane_id);
                self.path_suggestions.clear();
                self.path_suggestion_selection = None;
                self.navigate_to(path, NavigationMode::RecordHistory)
            }
            Message::PathSuggestionMoved(direction) => {
                self.move_search_or_path_suggestion_selection(direction)
            }
            Message::PathSuggestionCompleted(direction) => {
                self.complete_search_scope_or_path_suggestion(direction)
            }
            Message::PathInputStabilized(pane_id, request) => {
                self.load_stable_path_suggestions(pane_id, request)
            }
            Message::PathSuggestionsLoaded(pane_id, request, suggestions) => {
                self.accept_path_suggestions(pane_id, request, suggestions)
            }
            Message::SystemThemeDetected(theme) => {
                self.theme = theme;
                Task::none()
            }
            Message::UserConfigSaved(Ok(())) => Task::none(),
            Message::UserConfigSaved(Err(error)) => {
                self.error = Some(format!("Failed to save user configuration: {error}"));
                Task::none()
            }
            Message::ColumnWidthOverrideSaved(Ok(())) => Task::none(),
            Message::ColumnWidthOverrideSaved(Err(error)) => {
                self.error = Some(format!("Failed to save column width: {error}"));
                Task::none()
            }
            Message::SidebarBookmarksSaved(Ok(())) => Task::none(),
            Message::SidebarBookmarksSaved(Err(error)) => {
                self.error = Some(format!("Failed to save sidebar favorites: {error}"));
                Task::none()
            }
            Message::SearchOpened if self.is_trash_view => Task::none(),
            Message::SearchOpened => self.open_search(),
            Message::SearchInputChanged(query) => self.update_search_query(query),
            Message::SearchInputStabilized(request) => self.load_stable_search_matches(request),
            Message::SearchFocusRequested => {
                if self.search.is_some() {
                    search::focus_search_input_command()
                } else {
                    Task::none()
                }
            }
            Message::SearchMatchesLoaded(request, search) => {
                self.accept_search_matches(request, search)
            }
            Message::SearchIndexBuilt(root, outcome) => self.accept_search_index(root, outcome),
            Message::SearchMatchSelected(path) => self.activate_search_match(path),
            Message::SearchActivated => self.activate_selected_search_match(),
            Message::ExpandedDirectoryLoaded(pane_id, path, scan) => {
                self.accept_expanded_directory(pane_id, path, scan)
            }
            Message::ObservedDirectoryChanged(path) => self.reload_observed_directory(path),
            Message::ColumnSettingsToggled => {
                self.is_column_view_settings_open = !self.is_column_view_settings_open;
                self.context_menu = None;
                Task::none()
            }
            Message::ShowHiddenFilesToggled => self.toggle_show_hidden_files(),
            Message::TerminalEmulatorSelected(terminal_emulator) => {
                self.terminal_emulator = terminal_emulator;
                self.user_config.terminal_emulator = terminal_emulator;
                self.is_column_view_settings_open = false;
                self.persist_user_config_command()
            }
            Message::RenderingGpuPreferenceSelected(preference) => {
                self.select_rendering_gpu_preference(preference)
            }
            Message::RendererRestartNoticeDismissed => self.dismiss_renderer_restart_notice(),
            Message::CapturedWheelScrolled(delta) => Task::batch([
                self.show_scrollbars_temporarily(),
                self.handle_column_browser_wheel_scrolled(delta),
            ]),
            Message::ScrollbarAutoHideElapsed(generation) => {
                if generation == self.scrollbar_auto_hide_generation {
                    self.start_scrollbar_hide();
                }
                Task::none()
            }
            Message::WindowChromeAnimationTick => Task::batch([
                self.advance_scrollbar_animation(),
                self.advance_tab_bar_reveal_animation(),
                self.advance_tab_animations(),
                self.advance_sidebar_bookmark_motion(),
            ]),
            Message::ColumnScrolled(pane_id, directory, offset_y, height) => {
                self.activate_pane(pane_id);
                self.handle_column_scrolled(directory, offset_y, height)
            }
            Message::ColumnResizeStarted(pane_id, column_index) => {
                self.activate_pane(pane_id);
                self.start_column_resize_drag(column_index)
            }
            Message::OpenDirectoryInNewTab(pane_id, path) => {
                self.activate_pane(pane_id);
                Task::batch([
                    self.commit_rename_if_active(),
                    self.open_directory_in_new_tab(path),
                ])
            }
            Message::OpenTrashInNewTab(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.open_trash_in_new_tab()])
            }
            Message::TabPressed(pane_id, tab_id) => {
                self.activate_pane(pane_id);
                let rename_command = self.commit_rename_if_active();
                self.start_tab_drag(pane_id, tab_id);
                Task::batch([rename_command, self.select_tab(tab_id)])
            }
            Message::TabCloseRequested(pane_id, tab_id) => {
                self.activate_pane(pane_id);
                self.close_tab(tab_id)
            }
            Message::TabDragEntered(pane_id, tab_id) => {
                self.reorder_dragged_tab(pane_id, tab_id);
                Task::none()
            }
            Message::TabDragFinished => {
                self.finish_tab_drag();
                Task::batch([
                    self.finish_sidebar_bookmark_drag(),
                    self.finish_column_resize_drag_command(),
                    self.finish_drag_selection(),
                ])
            }
            Message::NavigateTo(path) => Task::batch([
                self.commit_rename_if_active(),
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]),
            Message::TrashOpened => Task::batch([
                self.commit_rename_if_active(),
                self.open_trash_view(NavigationMode::RecordHistory),
            ]),
            Message::Back => Task::batch([self.commit_rename_if_active(), self.navigate_back()]),
            Message::Forward => {
                Task::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            Message::PaneUp(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_up()])
            }
            Message::PaneBack(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_back()])
            }
            Message::PaneForward(pane_id) => {
                self.activate_pane(pane_id);
                Task::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            Message::RenameInputFocusChecked(is_focused) => {
                if is_focused {
                    Task::none()
                } else {
                    self.commit_rename_if_active()
                }
            }
            Message::RenameInputChanged(value) => {
                self.rename_input = value;
                Task::none()
            }
            Message::BeginRename(path) => {
                self.context_menu = None;
                self.select_path(path.clone());
                self.renaming = Some(path);
                let input_id = rename_input_id();
                Task::batch([
                    iced::widget::operation::focus(input_id.clone()),
                    iced::widget::operation::select_all(input_id),
                ])
            }
            Message::OpenTerminalHere(directory) => self.open_terminal_here(directory),
            Message::RenameSelected => self.commit_rename(),
            Message::UndoFileOperation => self.undo_file_operation(),
            Message::RedoFileOperation => self.redo_file_operation(),
            Message::CreateDirectory(directory) => self.create_directory_in(directory),
            Message::CreateEmptyFile(directory) => self.create_empty_file_in(directory),
            Message::TrashSelected => self.trash_selected(),
            Message::RestoreSelected => self.restore_selected(),
            Message::EmptyTrashRequested => self.empty_trash_requested(),
            Message::CopySelected => self.copy_selected(),
            Message::MoveSelected => self.move_selected(),
            Message::PastePending => self.paste_pending(),
            Message::FileClipboardWriteFinished(result) => self.accept_file_clipboard_write(result),
            Message::DesktopClipboardReadFinished {
                paste_directory,
                fallback_operation,
                content,
            } => self.accept_desktop_clipboard_paste(paste_directory, fallback_operation, content),
            Message::ClipboardFileCreated(result) => self.accept_clipboard_file_created(result),
            Message::PrimarySelectAllRequested => {
                text_input_shortcuts::select_focused_text_or_visible_files_command()
            }
            Message::TransferConflictsChecked {
                mode,
                transfers,
                conflicts,
            } => self.accept_transfer_conflicts_checked(mode, transfers, conflicts),
            Message::TransferConflictChoiceSelected(choice) => {
                self.resolve_transfer_conflict_choice(choice)
            }
            Message::TransferConflictApplyToAllToggled => {
                self.toggle_transfer_conflict_apply_to_all();
                Task::none()
            }
            Message::TransferConflictRenameInputChanged(value) => {
                self.update_transfer_conflict_rename(value);
                Task::none()
            }
            Message::TransferConflictRenameConfirmed => self.confirm_transfer_conflict_rename(),
            Message::TransferConflictRenameTargetChecked {
                state,
                transfer_position,
                target,
                available,
            } => self.accept_transfer_conflict_rename_target(
                state,
                transfer_position,
                target,
                available,
            ),
            Message::TransferConflictCancelRequested => {
                self.transfer_conflict = None;
                Task::none()
            }
            Message::SelectAll => self.select_all_visible(),
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        if self.search_window == Some(window) {
            view_search_window(self.search.as_ref(), self.scrollbar_visibility)
        } else if self.preview_window == Some(window) {
            view_preview_window(
                self.preview.as_ref(),
                self.text_preview_document.as_ref(),
                self.preview_size,
                self.audio_preview.as_ref(),
                self.video_preview.as_ref(),
                self.scrollbar_visibility,
            )
        } else {
            if !self.is_loading {
                startup_trace::mark_once("first_browser_view_after_initial_load");
            }
            view_browser(self)
        }
    }
}
