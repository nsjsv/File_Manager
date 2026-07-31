mod application_logs;
pub(crate) mod archive_creation;
pub(crate) mod archive_extraction;
pub(crate) mod archive_password;
mod batch_rename;
mod column_resize;
mod column_scroll;
mod config_persistence;
mod directory_recovery;
mod events;
mod file_operation_notifications;
mod file_operations;
mod global_error;
mod list_directory_summaries;
#[cfg(test)]
mod list_size_sorting_tests;
mod list_view_settings;
mod navigation;
mod network_connections;
mod network_settings;
mod open_with;
mod pane_drag;
pub(crate) mod panes;
mod path_suggestions;
mod paths;
mod persistence_feedback;
mod pointer_interactions;
mod preview_state;
mod properties;
mod remote_mounts;
mod rendering_settings;
mod runtime;
mod scrollbar;
mod search;
mod selection;
mod session_persistence;
mod session_restore;
mod shortcuts;
mod sidebar_bookmarks;
mod sidebar_devices;
mod sidebar_resize;
pub(crate) mod smooth_scroll;
mod startup;
mod startup_settings;
mod tabs;
mod text_input_shortcuts;
mod thumbnailing;
mod update;
mod view_modes;
mod wayland_dnd;
mod window_chrome;
mod window_control_settings;
mod windows;

pub(crate) use runtime::run;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use desktop_linux::{NetworkConnectionId, StorageDeviceId, TerminalEmulator, WaylandDndFileDrop};
use file_core::{DirectoryEntry, ScanOptions, TrashEntry};
use iced::event;
use iced::keyboard;
use iced::window;
use iced::{time, Element, Point, Subscription, Task, Theme};

use crate::animated_image_preview::animated_image_preview_subscription;
use crate::app::archive_creation::ArchiveCreationState;
use crate::app::archive_extraction::ArchiveExtractionState;
use crate::app::column_resize::ColumnResizeDrag;
use crate::app::events::global_event_message;
use crate::app::runtime::{
    directory_watch_subscription, sidebar_device_refresh_subscription, system_theme_command,
    wayland_file_dnd_subscription,
};
use crate::app::scrollbar::{ScrollbarState, SCROLLBAR_ANIMATION_INTERVAL};
use crate::app::sidebar_bookmarks::SidebarBookmarkMotionState;
use crate::app::sidebar_resize::SidebarResizeDrag;
use crate::app::smooth_scroll::MosScrollState;
use crate::app::tabs::{TabAnimationState, TabBarReveal};
use crate::app::windows::{
    default_preview_size, main_window_settings, MAIN_WINDOW_INITIAL_HEIGHT,
    MAIN_WINDOW_INITIAL_WIDTH,
};
use crate::command_line::ApplicationLaunchRequest;
use crate::commands::{
    ensure_search_service_command, file_operation_subscription, startup_environment_command,
    wayland_dnd_window_handle_command,
};
use crate::config;
use crate::config::UiLanguage;
use crate::localization;
use crate::model::search::SearchState;
use crate::model::{
    AddressBarTransition, AddressEditingSession, ApplicationLogViewState, AudioPreviewPlayback,
    BatchRenameState, BreadcrumbDropTargetBounds, BrowserPane, BrowserPaneId, BrowserPaneLayout,
    BrowserTab, BrowserViewMode, ColumnBrowserViewport, ColumnEntryBounds, ContextMenuState,
    DestructiveActionConfirmation, DirectoryLoadingPlaceholderEntry, ExpandedDirectory,
    FileDragState, FileDropPrompt, FilePropertiesState, IconGridViewport, ListColumnKind, Message,
    PaneDragPointerPress, PaneDragState, PendingOperation, PreviewSize, PreviewState,
    PreviewWindowProfile, ScrollbarRegion, SelectionMarquee, SettingsCategory,
    SidebarBookmarkDragState, SidebarBookmarkDropSlot, SidebarLocation,
    StartupDirectoryValidationRequest, TabDragState, TextPreviewDocument, TransferConflictState,
    VideoPreviewPlayback,
};
use crate::network_connections::{NetworkConnectionEditorState, NetworkConnectionState};
use crate::open_with::OpenWithState;
use crate::operation_history::FileOperationHistory;
use crate::operation_queue::FileOperationQueue;
use crate::shortcuts::ShortcutCaptureState;
use crate::sidebar_devices::SidebarDeviceState;
use crate::startup_trace;
use crate::thumbnail_cache::{ColumnViewport, ThumbnailCache};
use crate::video_preview::video_preview_subscription;
use crate::view::{
    separate_window_content, view_browser, view_preview_window, view_properties_window,
    view_settings_window, window_resize_frame,
};

const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(500);
const POINTER_DRAG_ACTIVATION_DISTANCE: f32 = 3.0;
const PREVIEW_TREE_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const AUDIO_PREVIEW_TICK_INTERVAL: Duration = Duration::from_millis(250);
const NETWORK_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const SEARCH_SERVICE_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APPLICATION_LOG_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) struct FileBrowser {
    pub(crate) home_dir: PathBuf,
    pub(crate) current_dir: PathBuf,
    pub(crate) is_trash_view: bool,
    pub(crate) entries: Vec<DirectoryEntry>,
    pub(crate) directory_loading_placeholder_entries: Vec<DirectoryLoadingPlaceholderEntry>,
    pub(crate) trash_entries: Vec<TrashEntry>,
    pub(crate) selected: Option<PathBuf>,
    selected_paths: HashSet<PathBuf>,
    pub(crate) hovered_entry: Option<PathBuf>,
    pub(crate) hovered_sidebar: Option<PathBuf>,
    pub(crate) hovered_sidebar_device: Option<StorageDeviceId>,
    pub(crate) hovered_network_connection: Option<NetworkConnectionId>,
    cursor_paste_directory: Option<PathBuf>,
    pub(crate) preview: Option<PreviewState>,
    remote_preview_download_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) text_preview_document: Option<TextPreviewDocument>,
    animated_image_preview_generation: u64,
    remote_preview_download_generation: u64,
    text_preview_generation: u64,
    directory_load_generation: u64,
    directory_load_cancel: Option<tokio_util::sync::CancellationToken>,
    pub(crate) audio_preview: Option<AudioPreviewPlayback>,
    pub(crate) video_preview: Option<VideoPreviewPlayback>,
    pub(crate) preview_size: PreviewSize,
    pending_preview_resize: Option<PreviewSize>,
    preview_window_profile: PreviewWindowProfile,
    main_window: window::Id,
    maximized_windows: HashSet<window::Id>,
    wayland_dnd: Option<wayland_dnd::WaylandDndRuntime>,
    preview_window: Option<window::Id>,
    focused_window: window::Id,
    system_focused_window: Option<window::Id>,
    pub(crate) thumbnail_cache: ThumbnailCache,
    pub(crate) column_browser_viewport: ColumnBrowserViewport,
    pub(crate) column_viewports: HashMap<PathBuf, ColumnViewport>,
    icon_grid_viewports: HashMap<BrowserPaneId, PaneIconGridViewport>,
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) open_with: Option<OpenWithState>,
    pub(crate) file_drop_prompt: Option<FileDropPrompt>,
    pub(crate) archive_creation: Option<ArchiveCreationState>,
    pub(crate) archive_extraction: Option<ArchiveExtractionState>,
    pub(crate) batch_rename: Option<BatchRenameState>,
    pub(crate) sidebar_locations: Vec<SidebarLocation>,
    pub(crate) sidebar_devices: SidebarDeviceState,
    pub(crate) network_connections: NetworkConnectionState,
    pub(crate) network_connection_editor: Option<NetworkConnectionEditorState>,
    pub(crate) sidebar_bookmark_drop_slot: Option<SidebarBookmarkDropSlot>,
    pub(crate) sidebar_bookmark_drag: Option<SidebarBookmarkDragState>,
    pub(crate) sidebar_bookmark_motion: HashMap<PathBuf, SidebarBookmarkMotionState>,
    pub(crate) sidebar_width: f32,
    sidebar_resize_drag: Option<SidebarResizeDrag>,
    pub(crate) renaming: Option<PathBuf>,
    pending_created_entry_rename: Option<PathBuf>,
    pub(crate) pending_operation: Option<PendingOperation>,
    pub(crate) transfer_conflict: Option<TransferConflictState>,
    pub(crate) destructive_action_confirmation: Option<DestructiveActionConfirmation>,
    settings_window: Option<window::Id>,
    properties_window: Option<window::Id>,
    pub(crate) properties: Option<FilePropertiesState>,
    properties_load_generation: u64,
    pub(crate) tabs: Vec<BrowserTab>,
    pub(crate) active_tab_id: usize,
    tab_bar_reveal: TabBarReveal,
    pub(crate) tab_animations: HashMap<usize, TabAnimationState>,
    pub(crate) panes: Vec<BrowserPane>,
    pub(crate) pane_layout: BrowserPaneLayout,
    tab_drag: Option<TabDragState>,
    pane_drag: Option<PaneDragState>,
    pane_drag_pointer_press: Option<PaneDragPointerPress>,
    pub(crate) selection_marquee: Option<SelectionMarquee>,
    pub(crate) file_drag: Option<FileDragState>,
    file_entry_bounds: Vec<ColumnEntryBounds>,
    breadcrumb_drop_target_bounds: Vec<BreadcrumbDropTargetBounds>,
    breadcrumb_drop_target_measurement_generation: u64,
    native_file_drag_target_measurement_generation: u64,
    pending_wayland_file_drop: Option<PendingWaylandFileDrop>,
    pub(crate) options: ScanOptions,
    application_launch_request: ApplicationLaunchRequest,
    user_config: config::UserConfig,
    pub(crate) max_preview_file_mib_input: String,
    pub(crate) max_preview_file_mib_error: Option<String>,
    pub(crate) startup_custom_directory_input: String,
    pub(crate) startup_custom_directory_error: Option<String>,
    pending_startup_directory_validation: Option<StartupDirectoryValidationRequest>,
    startup_directory_validation_generation: u64,
    pub(crate) rendering_gpu_preference: config::RenderingGpuPreference,
    pub(crate) renderer_restart_notice_visible: bool,
    pub(super) pending_renderer_restart_environment:
        Option<crate::startup_rendering::StartupRenderingEnvironment>,
    pub(crate) address_editing: Option<AddressEditingSession>,
    pub(crate) address_bar_transition: Option<AddressBarTransition>,
    next_address_editing_session_id: u64,
    pub(crate) column_width_overrides: HashMap<usize, f32>,
    column_width_reference_content_widths: HashMap<usize, f32>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) selected_settings_category: SettingsCategory,
    pub(crate) search: SearchState,
    pub(crate) deepest_open_column_directory: Option<PathBuf>,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) view_mode: BrowserViewMode,
    pub(crate) rename_input: String,
    rename_input_history: file_operations::RenameInputHistory,
    pub(crate) is_loading: bool,
    error: Option<String>,
    pub(crate) application_logs: ApplicationLogViewState,
    system_language: UiLanguage,
    pub(crate) cursor_position: Point,
    pub(crate) main_window_width: f32,
    pub(crate) main_window_height: f32,
    is_cursor_over_column_browser: bool,
    hovered_pane_id: Option<BrowserPaneId>,
    hovered_list_header_column: Option<(BrowserPaneId, ListColumnKind)>,
    keyboard_modifiers: keyboard::Modifiers,
    pub(crate) shortcut_capture: Option<ShortcutCaptureState>,
    selection_anchor: Option<PathBuf>,
    drag_selection_anchor: Option<PathBuf>,
    column_return_targets: HashMap<PathBuf, PathBuf>,
    pending_keyboard_column_focus: Option<PendingKeyboardColumnFocus>,
    column_resize_drag: Option<ColumnResizeDrag>,
    list_column_resize_drag: Option<crate::app::list_view_settings::ListColumnResizeDrag>,
    list_column_reorder_drag: Option<crate::app::list_view_settings::ListColumnReorderDrag>,
    window_control_reorder_drag:
        Option<crate::app::window_control_settings::WindowControlReorderDrag>,
    last_activation_click: Option<crate::model::LastActivationClick>,
    pub(crate) operation_queue: FileOperationQueue,
    operation_history: FileOperationHistory,
    list_directory_summary_cache: crate::model::ListDirectorySummaryCache,
    pending_browser_session_save: bool,
    last_browser_session_save: Option<std::time::Instant>,
    scrollbar: ScrollbarState,
    smooth_scroll: MosScrollState,
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    next_tab_id: usize,
    next_pane_id: u64,
    theme: Theme,
    is_shutting_down: bool,
}

struct PendingWaylandFileDrop {
    measurement_generation: u64,
    drop: WaylandDndFileDrop,
}

#[derive(Debug, Clone)]
struct PaneIconGridViewport {
    directory: PathBuf,
    viewport: IconGridViewport,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingKeyboardColumnFocus {
    pub(crate) pane_id: BrowserPaneId,
    pub(crate) directory: PathBuf,
    pub(crate) preferred_child: Option<PathBuf>,
}

impl FileBrowser {
    pub(crate) fn user_config(&self) -> &config::UserConfig {
        &self.user_config
    }

    pub(crate) fn active_language(&self) -> UiLanguage {
        self.user_config
            .language_setting
            .resolve(self.system_language)
    }

    fn refresh_current_language(&self) {
        localization::set_current_language(self.active_language());
    }

    pub(crate) fn file_operation_verification(&self) -> file_core::FileOperationVerification {
        self.user_config.file_operation_verification
    }

    pub(crate) fn remote_list_thumbnail_downloads_enabled(&self) -> bool {
        self.user_config.network_list_thumbnail_downloads_enabled
    }

    pub(crate) fn max_preview_file_bytes(&self) -> u64 {
        self.user_config.max_preview_file_bytes
    }

    fn boot(application_launch_request: ApplicationLaunchRequest) -> (Self, Task<Message>) {
        let (main_window, open_main_window) = window::open(main_window_settings());
        let user_config = config::ui_thread_startup_config();
        let (browser, initial_tasks) =
            Self::new_with_main_window(user_config, main_window, application_launch_request);

        let open_main_window = open_main_window.then(wayland_dnd_window_handle_command);

        (browser, Task::batch([open_main_window, initial_tasks]))
    }

    #[cfg(test)]
    pub(crate) fn new(user_config: config::UserConfig) -> (Self, Task<Message>) {
        Self::new_with_launch_request(user_config, ApplicationLaunchRequest::ConfiguredStartup)
    }

    #[cfg(test)]
    pub(crate) fn new_with_launch_request(
        user_config: config::UserConfig,
        application_launch_request: ApplicationLaunchRequest,
    ) -> (Self, Task<Message>) {
        Self::new_with_main_window(
            user_config,
            window::Id::unique(),
            application_launch_request,
        )
    }

    fn new_with_main_window(
        user_config: config::UserConfig,
        main_window: window::Id,
        application_launch_request: ApplicationLaunchRequest,
    ) -> (Self, Task<Message>) {
        startup_trace::mark_once("file_browser_new_started");
        let placeholder_dir = PathBuf::from("/");
        let options = ScanOptions::default();
        let initial_view_mode = user_config.browser_view_mode;
        let mut search = SearchState::new();
        let initial_search_service_request = search.service.begin_initial_status_request();
        let mut initial_tab = BrowserTab::directory(0, placeholder_dir.clone());
        initial_tab.view_mode = initial_view_mode;
        let initial_pane = BrowserPane {
            id: BrowserPaneId::PRIMARY,
            current_dir: placeholder_dir.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            directory_loading_placeholder_entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            deepest_open_column_directory: None,
            expanded_directories: HashMap::new(),
            view_mode: initial_view_mode,
            column_browser_viewport: ColumnBrowserViewport::default(),
            column_viewports: HashMap::new(),
            tabs: vec![initial_tab.clone()],
            active_tab_id: initial_tab.id,
            directory_load_generation: 0,
            directory_load_cancel: None,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            is_loading: true,
        };
        let mut browser = Self {
            home_dir: placeholder_dir.clone(),
            current_dir: placeholder_dir.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            directory_loading_placeholder_entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            hovered_entry: None,
            hovered_sidebar: None,
            hovered_sidebar_device: None,
            hovered_network_connection: None,
            cursor_paste_directory: None,
            preview: None,
            remote_preview_download_cancel: None,
            text_preview_document: None,
            animated_image_preview_generation: 0,
            remote_preview_download_generation: 0,
            text_preview_generation: 0,
            directory_load_generation: 0,
            directory_load_cancel: None,
            audio_preview: None,
            video_preview: None,
            preview_size: default_preview_size(PreviewWindowProfile::Regular),
            pending_preview_resize: None,
            preview_window_profile: PreviewWindowProfile::Regular,
            main_window,
            maximized_windows: HashSet::new(),
            wayland_dnd: None,
            preview_window: None,
            focused_window: main_window,
            system_focused_window: None,
            thumbnail_cache: ThumbnailCache::new(user_config.thumbnail_cache_dir.clone()),
            column_browser_viewport: ColumnBrowserViewport::default(),
            column_viewports: HashMap::new(),
            icon_grid_viewports: HashMap::new(),
            context_menu: None,
            open_with: None,
            file_drop_prompt: None,
            archive_creation: None,
            archive_extraction: None,
            batch_rename: None,
            sidebar_locations: Vec::new(),
            sidebar_devices: SidebarDeviceState::loading(),
            network_connections: NetworkConnectionState::default(),
            network_connection_editor: None,
            sidebar_bookmark_drop_slot: None,
            sidebar_bookmark_drag: None,
            sidebar_bookmark_motion: HashMap::new(),
            sidebar_width: user_config.sidebar_width,
            sidebar_resize_drag: None,
            renaming: None,
            pending_created_entry_rename: None,
            pending_operation: None,
            transfer_conflict: None,
            destructive_action_confirmation: None,
            settings_window: None,
            properties_window: None,
            properties: None,
            properties_load_generation: 0,
            tabs: vec![initial_tab],
            active_tab_id: 0,
            tab_bar_reveal: TabBarReveal::default(),
            tab_animations: HashMap::new(),
            panes: vec![initial_pane],
            pane_layout: BrowserPaneLayout::Single {
                active: BrowserPaneId::PRIMARY,
            },
            tab_drag: None,
            pane_drag: None,
            pane_drag_pointer_press: None,
            selection_marquee: None,
            file_drag: None,
            file_entry_bounds: Vec::new(),
            breadcrumb_drop_target_bounds: Vec::new(),
            breadcrumb_drop_target_measurement_generation: 0,
            native_file_drag_target_measurement_generation: 0,
            pending_wayland_file_drop: None,
            options: options.clone(),
            application_launch_request,
            user_config: user_config.clone(),
            max_preview_file_mib_input: config::max_preview_file_mib(
                user_config.max_preview_file_bytes,
            )
            .to_string(),
            max_preview_file_mib_error: None,
            startup_custom_directory_input: user_config
                .startup_custom_directory
                .to_string_lossy()
                .into_owned(),
            startup_custom_directory_error: None,
            pending_startup_directory_validation: None,
            startup_directory_validation_generation: 0,
            rendering_gpu_preference: user_config.rendering_gpu_preference,
            renderer_restart_notice_visible: false,
            pending_renderer_restart_environment: None,
            address_editing: None,
            address_bar_transition: None,
            next_address_editing_session_id: 1,
            column_width_overrides: HashMap::new(),
            column_width_reference_content_widths: HashMap::new(),
            terminal_emulator: user_config.terminal_emulator,
            selected_settings_category: SettingsCategory::General,
            search,
            deepest_open_column_directory: None,
            expanded_directories: HashMap::new(),
            view_mode: initial_view_mode,
            rename_input: String::new(),
            rename_input_history: file_operations::RenameInputHistory::default(),
            is_loading: true,
            error: None,
            application_logs: ApplicationLogViewState::new(
                crate::runtime_logging::journald_initialization_warning(),
            ),
            system_language: UiLanguage::English,
            cursor_position: Point::new(0.0, 0.0),
            main_window_width: MAIN_WINDOW_INITIAL_WIDTH,
            main_window_height: MAIN_WINDOW_INITIAL_HEIGHT,
            is_cursor_over_column_browser: false,
            hovered_pane_id: None,
            hovered_list_header_column: None,
            keyboard_modifiers: keyboard::Modifiers::default(),
            shortcut_capture: None,
            selection_anchor: None,
            drag_selection_anchor: None,
            column_return_targets: HashMap::new(),
            pending_keyboard_column_focus: None,
            column_resize_drag: None,
            list_column_resize_drag: None,
            list_column_reorder_drag: None,
            window_control_reorder_drag: None,
            last_activation_click: None,
            operation_queue: FileOperationQueue::new(),
            operation_history: FileOperationHistory::new(),
            list_directory_summary_cache: crate::model::ListDirectorySummaryCache::default(),
            pending_browser_session_save: false,
            last_browser_session_save: None,
            scrollbar: ScrollbarState::default(),
            smooth_scroll: MosScrollState::default(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            next_tab_id: 1,
            next_pane_id: 1,
            theme: Theme::Light,
            is_shutting_down: false,
        };
        browser.refresh_current_language();
        browser.refresh_column_width_reference_content_widths();
        startup_trace::mark_once("file_browser_new_ready");
        (
            browser,
            Task::batch([
                startup_environment_command(),
                system_theme_command(),
                ensure_search_service_command(initial_search_service_request),
            ]),
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
        subscriptions.push(sidebar_device_refresh_subscription());
        if let Some(runtime) = &self.wayland_dnd {
            subscriptions.push(wayland_file_dnd_subscription(
                runtime.window_handle,
                runtime.controller.clone(),
            ));
        }

        if !self.is_loading && !self.is_trash_view {
            subscriptions.push(directory_watch_subscription(self.current_dir.clone()));
            subscriptions.extend(
                self.expanded_directories
                    .keys()
                    .cloned()
                    .map(directory_watch_subscription),
            );
        }

        if self.settings_window.is_some()
            && self.selected_settings_category == crate::model::SettingsCategory::Search
        {
            subscriptions.push(
                time::every(SEARCH_SERVICE_STATUS_REFRESH_INTERVAL)
                    .map(|_| Message::SearchServiceStatusRefreshRequested),
            );
        }

        if self.settings_window.is_some()
            && self.selected_settings_category == crate::model::SettingsCategory::Logs
        {
            subscriptions.push(
                time::every(APPLICATION_LOG_REFRESH_INTERVAL)
                    .map(|_| Message::ApplicationLogsRefreshRequested),
            );
        }

        if !self.network_connections.entries.is_empty() {
            subscriptions.push(time::every(NETWORK_STATUS_REFRESH_INTERVAL).map(|_| {
                Message::NetworkConnection(
                    crate::network_connections::NetworkConnectionMessage::StatusRefreshRequested,
                )
            }));
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
            || self.smooth_scroll_animation_is_active()
            || self.address_bar_transition_is_active()
            || self.tab_bar_reveal_animation_is_active()
            || self.tab_animation_is_active()
            || self.list_directory_animation_is_active()
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

        if let Some((path, generation, position)) = self.active_animated_image_preview_stream() {
            subscriptions.push(animated_image_preview_subscription(
                path, generation, position,
            ));
        }

        if let Some((path, generation, position)) = self.active_video_preview_stream() {
            subscriptions.push(video_preview_subscription(path, generation, position));
        }

        Subscription::batch(subscriptions)
    }

    fn theme(&self, _window: window::Id) -> Theme {
        self.theme.clone()
    }

    fn view_with_separate_window_chrome<'a>(
        &'a self,
        window: window::Id,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let frame_state = self.window_frame_state(window);
        let content = separate_window_content(
            self.window_title(window),
            content,
            &self.user_config.window_controls,
            window,
            frame_state,
        );
        window_resize_frame(content, window, frame_state)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        if self.settings_window == Some(window) {
            self.view_with_separate_window_chrome(window, view_settings_window(self))
        } else if self.properties_window == Some(window) {
            let content = view_properties_window(
                self.properties.as_ref(),
                self.scrollbar_visibility_for(&ScrollbarRegion::Properties),
            );
            self.view_with_separate_window_chrome(window, content)
        } else if self.preview_window == Some(window) {
            let content = view_preview_window(
                self.preview.as_ref(),
                self.text_preview_document.as_ref(),
                self.preview_size,
                self.audio_preview.as_ref(),
                self.video_preview.as_ref(),
                self.scrollbar_visibility_for(&ScrollbarRegion::PreviewDirectory),
                self.scrollbar_visibility_for(&ScrollbarRegion::PreviewArchive),
                self.scrollbar_visibility_for(&ScrollbarRegion::MarkdownPreview),
            );
            self.view_with_separate_window_chrome(window, content)
        } else if window == self.main_window {
            startup_trace::mark_once("first_main_window_view");
            if !self.is_loading {
                startup_trace::mark_once("first_browser_view_after_initial_load");
            }
            view_browser(self)
        } else {
            iced::widget::container(crate::typography::readable_text("Closing window..."))
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .into()
        }
    }
}
