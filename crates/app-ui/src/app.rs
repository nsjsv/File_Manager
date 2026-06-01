mod column_scroll;
mod events;
mod file_operations;
mod navigation;
mod paths;
mod preview_state;
mod search;
mod selection;
mod startup;
mod tabs;
mod text_input_shortcuts;
mod thumbnailing;
mod windows;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use file_core::{watch_directory, DirectoryEntry, ScanOptions, TrashEntry};
use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Focusable, Operation, Outcome};
use iced::event;
use iced::futures::SinkExt;
use iced::keyboard;
use iced::multi_window::Application;
use iced::widget::text_input;
use iced::window;
use iced::{
    executor, time, Command, Element, Point, Rectangle, Settings, Size, Subscription, Theme,
};

use crate::app::events::{global_event_message, system_theme};
use crate::commands::{
    file_operation_subscription, initial_load_command, save_user_config_command,
};
use crate::config;
use crate::model::{
    AudioPreviewPlayback, BrowserTab, ColumnViewMode, ContextMenuState, ExpandedDirectory,
    FileDragState, Message, NavigationMode, OperationQueuePanelMode, PendingOperation, PreviewSize,
    PreviewState, PreviewWindowProfile, SearchIndexRuntime, SearchState, SelectionMarquee,
    SidebarLocation, TransferConflictState,
};
use crate::operation_queue::FileOperationQueue;
use crate::startup_trace;
use crate::thumbnail_cache::{ColumnViewport, ThumbnailCache};
use crate::video_preview::video_preview_subscription;
use crate::view::{rename_input_id, view_browser, view_preview_window, view_search_window};

const DOUBLE_CLICK_THRESHOLD: Duration = Duration::from_millis(500);
const DIRECTORY_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
const DIRECTORY_WATCH_CHANNEL_SIZE: usize = 8;
const OPERATION_QUEUE_AUTO_HIDE_DURATION: Duration = Duration::from_secs(5);
const PREVIEW_TREE_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const AUDIO_PREVIEW_TICK_INTERVAL: Duration = Duration::from_millis(250);
const COLUMN_BROWSER_WHEEL_LINE_PIXELS: f32 = 60.0;
const DEFAULT_PREVIEW_WIDTH: f32 = 720.0;
const DEFAULT_PREVIEW_HEIGHT: f32 = 440.0;
const MIN_PREVIEW_WIDTH: f32 = 420.0;
const MIN_PREVIEW_HEIGHT: f32 = 260.0;
const MAX_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_PREVIEW_HEIGHT: f32 = 760.0;
const DEFAULT_AUDIO_PREVIEW_WIDTH: f32 = 780.0;
const DEFAULT_AUDIO_PREVIEW_HEIGHT: f32 = 168.0;
const MIN_AUDIO_PREVIEW_WIDTH: f32 = 560.0;
const MIN_AUDIO_PREVIEW_HEIGHT: f32 = 136.0;
const MAX_AUDIO_PREVIEW_WIDTH: f32 = 1080.0;
const MAX_AUDIO_PREVIEW_HEIGHT: f32 = 240.0;
const DEFAULT_SEARCH_WIDTH: f32 = 680.0;
const DEFAULT_SEARCH_HEIGHT: f32 = 460.0;
const MIN_SEARCH_WIDTH: f32 = 520.0;
const MIN_SEARCH_HEIGHT: f32 = 360.0;
const MAIN_WINDOW_APP_ID: &str = "file-manager";
const SEARCH_WINDOW_APP_ID: &str = "file-manager-search";
const PREVIEW_WINDOW_APP_ID: &str = "file-manager-preview";

fn directory_watch_subscription(path: PathBuf) -> Subscription<Message> {
    iced::subscription::channel(
        ("directory-watch", path.clone()),
        DIRECTORY_WATCH_CHANNEL_SIZE,
        |mut output| async move {
            if let Ok(mut watcher) = watch_directory(path, DIRECTORY_WATCH_DEBOUNCE) {
                while let Some(change) = watcher.recv().await {
                    if output
                        .send(Message::ObservedDirectoryChanged(change.path))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }

            iced::futures::future::pending().await
        },
    )
}

fn system_theme_command() -> Command<Message> {
    Command::perform(
        async {
            let theme = tokio::task::spawn_blocking(system_theme)
                .await
                .unwrap_or(Theme::Light);
            startup_trace::mark_once("system_theme_detected");
            theme
        },
        Message::SystemThemeDetected,
    )
}

fn operation_queue_auto_hide_command(generation: u64) -> Command<Message> {
    Command::perform(
        async move {
            tokio::time::sleep(OPERATION_QUEUE_AUTO_HIDE_DURATION).await;
            generation
        },
        Message::FileOperationAutoHideElapsed,
    )
}

fn main_window_settings() -> window::Settings {
    let mut settings = window::Settings {
        size: Size::new(1180.0, 680.0),
        ..window::Settings::default()
    };
    settings.platform_specific.application_id = MAIN_WINDOW_APP_ID.to_owned();
    settings
}

#[derive(Debug, Clone, Copy)]
struct ColumnResizeDrag {
    cursor_start_x: f32,
    width_start: f32,
}

struct RenameInputFocusCheck {
    target: advanced_widget::Id,
    is_focused: bool,
}

impl RenameInputFocusCheck {
    fn new(target: text_input::Id) -> Self {
        Self {
            target: target.into(),
            is_focused: false,
        }
    }
}

impl Operation<Message> for RenameInputFocusCheck {
    fn container(
        &mut self,
        _id: Option<&advanced_widget::Id>,
        _bounds: Rectangle,
        operate_on_children: &mut dyn FnMut(&mut dyn Operation<Message>),
    ) {
        operate_on_children(self);
    }

    fn focusable(&mut self, state: &mut dyn Focusable, id: Option<&advanced_widget::Id>) {
        if id == Some(&self.target) {
            self.is_focused = state.is_focused();
        }
    }

    fn finish(&self) -> Outcome<Message> {
        Outcome::Some(Message::RenameInputFocusChecked(self.is_focused))
    }
}

fn rename_input_focus_check_command() -> Command<Message> {
    Command::widget(RenameInputFocusCheck::new(rename_input_id()))
}

pub(crate) fn run() -> iced::Result {
    FileBrowser::run(Settings {
        window: main_window_settings(),
        ..Settings::default()
    })
}

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
    pub(crate) preview: Option<PreviewState>,
    pub(crate) audio_preview: Option<AudioPreviewPlayback>,
    pub(crate) preview_size: PreviewSize,
    preview_window_profile: PreviewWindowProfile,
    preview_window: Option<window::Id>,
    focused_window: window::Id,
    pub(crate) thumbnail_cache: ThumbnailCache,
    pub(crate) column_viewports: HashMap<PathBuf, ColumnViewport>,
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) sidebar_locations: Vec<SidebarLocation>,
    pub(crate) renaming: Option<PathBuf>,
    pending_created_entry_rename: Option<PathBuf>,
    pub(crate) pending_operation: Option<PendingOperation>,
    pub(crate) transfer_conflict: Option<TransferConflictState>,
    pub(crate) search: Option<SearchState>,
    search_window: Option<window::Id>,
    pub(crate) search_index: SearchIndexRuntime,
    pub(crate) tabs: Vec<BrowserTab>,
    pub(crate) active_tab_id: usize,
    tab_drag_id: Option<usize>,
    pub(crate) selection_marquee: Option<SelectionMarquee>,
    pub(crate) file_drag: Option<FileDragState>,
    pub(crate) options: ScanOptions,
    user_config: config::UserConfig,
    pub(crate) path_input: String,
    pub(crate) path_suggestions: Vec<PathBuf>,
    pub(crate) path_suggestion_selection: Option<usize>,
    pub(crate) column_view_mode: ColumnViewMode,
    pub(crate) column_fixed_count: usize,
    pub(crate) unbounded_column_width: f32,
    pub(crate) is_column_view_settings_open: bool,
    pub(crate) expanded_directories: HashMap<PathBuf, ExpandedDirectory>,
    pub(crate) rename_input: String,
    pub(crate) is_loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) cursor_position: Point,
    is_cursor_over_column_browser: bool,
    keyboard_modifiers: keyboard::Modifiers,
    selection_anchor: Option<PathBuf>,
    drag_selection_anchor: Option<PathBuf>,
    column_resize_drag: Option<ColumnResizeDrag>,
    last_click: Option<crate::model::LastClick>,
    pub(crate) operation_queue: FileOperationQueue,
    pub(crate) operation_queue_panel_mode: OperationQueuePanelMode,
    operation_queue_auto_hide_generation: u64,
    pending_search_reveal: Option<PathBuf>,
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    next_tab_id: usize,
    theme: Theme,
    is_shutting_down: bool,
}

impl Application for FileBrowser {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;
    type Theme = Theme;

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        startup_trace::mark_once("file_browser_new_started");
        let placeholder_dir = PathBuf::from("/");
        let options = ScanOptions::default();
        let user_config = config::default_user_config();
        let browser = Self {
            current_dir: placeholder_dir.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            hovered_entry: None,
            hovered_sidebar: None,
            cursor_paste_directory: None,
            preview: None,
            audio_preview: None,
            preview_size: PreviewSize {
                width: DEFAULT_PREVIEW_WIDTH,
                height: DEFAULT_PREVIEW_HEIGHT,
            },
            preview_window_profile: PreviewWindowProfile::Regular,
            preview_window: None,
            focused_window: window::Id::MAIN,
            thumbnail_cache: ThumbnailCache::new(user_config.thumbnail_cache_dir.clone()),
            column_viewports: HashMap::new(),
            context_menu: None,
            sidebar_locations: Vec::new(),
            renaming: None,
            pending_created_entry_rename: None,
            pending_operation: None,
            transfer_conflict: None,
            search: None,
            search_window: None,
            search_index: SearchIndexRuntime::new(PathBuf::new()),
            tabs: vec![BrowserTab {
                id: 0,
                directory: placeholder_dir.clone(),
                is_trash_view: false,
                entries: Vec::new(),
                trash_entries: Vec::new(),
                selected: None,
                selected_paths: HashSet::new(),
                selection_anchor: None,
                expanded_directories: HashMap::new(),
                back_stack: Vec::new(),
                forward_stack: Vec::new(),
            }],
            active_tab_id: 0,
            tab_drag_id: None,
            selection_marquee: None,
            file_drag: None,
            options: options.clone(),
            user_config: user_config.clone(),
            path_input: String::new(),
            path_suggestions: Vec::new(),
            path_suggestion_selection: None,
            column_view_mode: user_config.column_view_mode,
            column_fixed_count: user_config.column_fixed_count,
            unbounded_column_width: user_config.unbounded_column_width,
            is_column_view_settings_open: false,
            expanded_directories: HashMap::new(),
            rename_input: String::new(),
            is_loading: true,
            error: None,
            cursor_position: Point::new(0.0, 0.0),
            is_cursor_over_column_browser: false,
            keyboard_modifiers: keyboard::Modifiers::default(),
            selection_anchor: None,
            drag_selection_anchor: None,
            column_resize_drag: None,
            last_click: None,
            operation_queue: FileOperationQueue::new(),
            operation_queue_panel_mode: OperationQueuePanelMode::PassivePreview,
            operation_queue_auto_hide_generation: 0,
            pending_search_reveal: None,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            next_tab_id: 1,
            theme: Theme::Light,
            is_shutting_down: false,
        };
        startup_trace::mark_once("file_browser_new_ready");
        (
            browser,
            Command::batch([initial_load_command(), system_theme_command()]),
        )
    }

    fn title(&self, window: window::Id) -> String {
        self.window_title(window)
    }

    fn subscription(&self) -> Subscription<Self::Message> {
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

        if self.audio_preview_is_active() {
            subscriptions
                .push(time::every(AUDIO_PREVIEW_TICK_INTERVAL).map(|_| Message::AudioPreviewTick));
        }

        if let Some(path) = self.active_video_preview_path() {
            subscriptions.push(video_preview_subscription(path));
        }

        Subscription::batch(subscriptions)
    }

    fn theme(&self, _window: window::Id) -> Self::Theme {
        self.theme.clone()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::InitialLoadFinished(initial_load) => self.accept_initial_load(initial_load),
            Message::Loaded(Ok(scan)) => self.accept_directory_scan(scan),
            Message::Loaded(Err(error)) => {
                self.is_loading = false;
                self.error = Some(error);
                Command::none()
            }
            Message::TrashLoaded(Ok(scan)) => self.accept_trash_scan(scan),
            Message::TrashLoaded(Err(error)) => {
                self.is_loading = false;
                self.error = Some(error);
                Command::none()
            }
            Message::OpenFileFinished(Ok(())) => {
                self.error = None;
                Command::none()
            }
            Message::OpenFileFinished(Err(error)) => {
                self.error = Some(error);
                Command::none()
            }
            Message::PreviewLoaded(path, preview_outcome) => {
                self.accept_preview(path, preview_outcome)
            }
            Message::AudioPreviewPlaybackToggled => self.toggle_audio_preview_playback(),
            Message::AudioPreviewStopRequested => self.stop_audio_preview_playback(),
            Message::AudioPreviewStarted(path, playback_outcome) => {
                self.accept_audio_preview_started(path, playback_outcome)
            }
            Message::AudioPreviewSeekRequested(position) => {
                self.seek_audio_preview_playback(position)
            }
            Message::AudioPreviewVolumeChanged(volume) => self.change_audio_preview_volume(volume),
            Message::AudioPreviewTick => self.update_audio_preview_playback(),
            Message::VideoPreviewFrameLoaded(frame) => self.accept_video_preview_frame(frame),
            Message::VideoPreviewFailed(path, error) => {
                self.accept_video_preview_error(path, error)
            }
            Message::FileOperationProgressed(task_id, progress) => {
                if let Some(error) = self.operation_queue.update_progress(task_id, progress) {
                    self.error = Some(error);
                }
                Command::none()
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
                Command::none()
            }
            Message::FileOperationAutoHideElapsed(generation) => {
                if generation == self.operation_queue_auto_hide_generation {
                    self.operation_queue.close_panel();
                    self.operation_queue_panel_mode = OperationQueuePanelMode::PassivePreview;
                }
                Command::none()
            }
            Message::FileOperationPauseToggled(task_id) => {
                if let Some(error) = self.operation_queue.toggle_pause(task_id) {
                    self.error = Some(error);
                }
                Command::none()
            }
            Message::FileOperationCancelRequested(task_id) => {
                if let Some(error) = self.operation_queue.cancel(task_id) {
                    self.error = Some(error);
                }
                Command::none()
            }
            Message::PreviewTreeDirectoryToggled(entry_id) => {
                self.toggle_preview_tree_directory(entry_id)
            }
            Message::PreviewTreeAnimationTick => self.advance_preview_tree_animation(),
            Message::ThumbnailBatchLoaded(outcomes) => self.accept_thumbnail_batch(outcomes),
            Message::ColumnEntryClicked(path) => self.handle_column_entry_clicked(path),
            Message::ColumnBlankClicked(path) => self.handle_column_blank_clicked(path),
            Message::EntryReleased => {
                self.finish_tab_drag();
                Command::batch([
                    self.finish_column_resize_drag_command(),
                    self.finish_drag_selection(),
                ])
            }
            Message::EntryRightClicked(path) => self.handle_entry_right_clicked(path),
            Message::EntryHovered(path) => self.handle_entry_hovered(path),
            Message::EntryHoverCleared(path) => self.handle_entry_hover_cleared(path),
            Message::DropTargetHovered(directory) => self.handle_drop_target_hovered(directory),
            Message::DropTargetHoverCleared(directory) => {
                self.handle_drop_target_hover_cleared(directory)
            }
            Message::BlankAreaPressed => self.start_selection_marquee(),
            Message::BlankAreaRightClicked(directory) => {
                self.handle_blank_area_right_clicked(directory)
            }
            Message::SidebarHovered(path) => {
                self.hovered_sidebar = Some(path);
                self.cursor_paste_directory = None;
                Command::none()
            }
            Message::SidebarHoverCleared(path) => {
                if self.hovered_sidebar.as_ref() == Some(&path) {
                    self.hovered_sidebar = None;
                }
                Command::none()
            }
            Message::CursorMoved(position) => {
                self.cursor_position = position;
                self.update_file_drag(position);
                self.update_column_resize_drag(position);
                self.update_selection_marquee(position);
                Command::none()
            }
            Message::ColumnBrowserCursorEntered => {
                self.is_cursor_over_column_browser = true;
                Command::none()
            }
            Message::ColumnBrowserCursorExited => {
                self.is_cursor_over_column_browser = false;
                self.clear_cursor_paste_target()
            }
            Message::KeyboardModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers;
                Command::none()
            }
            Message::DragSelectionFinished => {
                self.finish_tab_drag();
                Command::batch([
                    self.finish_column_resize_drag_command(),
                    self.finish_drag_selection(),
                ])
            }
            Message::DismissFloating => self.dismiss_floating(),
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
            Message::RequestPreview => self.request_preview(),
            Message::PathInputChanged(value) => self.update_path_input(value),
            Message::PathInputSubmitted => self.submit_path_input(),
            Message::PathSuggestionSelected(path) => {
                self.path_suggestions.clear();
                self.path_suggestion_selection = None;
                self.navigate_to(path, NavigationMode::RecordHistory)
            }
            Message::PathSuggestionMoved(direction) => {
                if self.search.is_some() {
                    return self.move_search_selection(direction);
                }
                self.move_path_suggestion_selection(direction);
                Command::none()
            }
            Message::PathSuggestionCompleted(direction) => {
                if self.search.is_some() {
                    self.toggle_search_scope()
                } else {
                    self.complete_path_suggestion(direction)
                }
            }
            Message::PathSuggestionsLoaded(query, suggestions) => {
                if query == self.path_input {
                    self.path_suggestions = suggestions;
                    self.normalize_path_suggestion_selection();
                }
                Command::none()
            }
            Message::SystemThemeDetected(theme) => {
                self.theme = theme;
                Command::none()
            }
            Message::UserConfigSaved(Ok(())) => Command::none(),
            Message::UserConfigSaved(Err(error)) => {
                self.error = Some(format!("Failed to save user configuration: {error}"));
                Command::none()
            }
            Message::SearchOpened if self.is_trash_view => Command::none(),
            Message::SearchOpened => self.open_search(),
            Message::SearchInputChanged(query) => self.update_search_query(query),
            Message::SearchFocusRequested => {
                if self.search.is_some() {
                    search::focus_search_input_command()
                } else {
                    Command::none()
                }
            }
            Message::SearchMatchesLoaded(request, search) => {
                self.accept_search_matches(request, search)
            }
            Message::SearchIndexBuilt(root, outcome) => self.accept_search_index(root, outcome),
            Message::SearchMatchSelected(path) => self.activate_search_match(path),
            Message::SearchActivated => self.activate_selected_search_match(),
            Message::ExpandedDirectoryLoaded(path, scan) => {
                self.accept_expanded_directory(path, scan)
            }
            Message::ObservedDirectoryChanged(path) => self.reload_observed_directory(path),
            Message::ColumnSettingsToggled => {
                self.is_column_view_settings_open = !self.is_column_view_settings_open;
                self.context_menu = None;
                Command::none()
            }
            Message::ShowHiddenFilesToggled => self.toggle_show_hidden_files(),
            Message::ColumnViewModeSelected(mode) => {
                self.column_view_mode = mode;
                self.user_config.column_view_mode = mode;
                self.finish_column_resize_drag();
                self.is_column_view_settings_open = false;
                self.persist_user_config_command()
            }
            Message::ColumnFixedCountSelected(count) => {
                self.column_view_mode = ColumnViewMode::Fixed;
                self.user_config.column_view_mode = ColumnViewMode::Fixed;
                self.finish_column_resize_drag();
                let count = config::normalize_column_fixed_count(count);
                self.column_fixed_count = count;
                self.user_config.column_fixed_count = count;
                self.is_column_view_settings_open = false;
                self.persist_user_config_command()
            }
            Message::ColumnBrowserWheelScrolled(delta) => {
                self.handle_column_browser_wheel_scrolled(delta)
            }
            Message::ColumnScrolled(directory, offset_y, height) => {
                self.handle_column_scrolled(directory, offset_y, height)
            }
            Message::ColumnResizeStarted => self.start_column_resize_drag(),
            Message::OpenDirectoryInNewTab(path) => Command::batch([
                self.commit_rename_if_active(),
                self.open_directory_in_new_tab(path),
            ]),
            Message::OpenTrashInNewTab => {
                Command::batch([self.commit_rename_if_active(), self.open_trash_in_new_tab()])
            }
            Message::TabPressed(tab_id) => {
                let rename_command = self.commit_rename_if_active();
                self.start_tab_drag(tab_id);
                Command::batch([rename_command, self.select_tab(tab_id)])
            }
            Message::TabCloseRequested(tab_id) => self.close_tab(tab_id),
            Message::TabDragEntered(tab_id) => {
                self.reorder_dragged_tab(tab_id);
                Command::none()
            }
            Message::TabDragFinished => {
                self.finish_tab_drag();
                Command::none()
            }
            Message::NavigateTo(path) => Command::batch([
                self.commit_rename_if_active(),
                self.navigate_to(path, NavigationMode::RecordHistory),
            ]),
            Message::TrashOpened => Command::batch([
                self.commit_rename_if_active(),
                self.open_trash_view(NavigationMode::RecordHistory),
            ]),
            Message::Up => Command::batch([self.commit_rename_if_active(), self.navigate_up()]),
            Message::Back => Command::batch([self.commit_rename_if_active(), self.navigate_back()]),
            Message::Forward => {
                Command::batch([self.commit_rename_if_active(), self.navigate_forward()])
            }
            Message::RenameInputFocusChecked(is_focused) => {
                if is_focused {
                    Command::none()
                } else {
                    self.commit_rename_if_active()
                }
            }
            Message::RenameInputChanged(value) => {
                self.rename_input = value;
                Command::none()
            }
            Message::BeginRename(path) => {
                self.context_menu = None;
                self.select_path(path.clone());
                self.renaming = Some(path);
                let input_id = rename_input_id();
                Command::batch([
                    text_input::focus(input_id.clone()),
                    text_input::select_all(input_id),
                ])
            }
            Message::RenameSelected => self.commit_rename(),
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
                Command::none()
            }
            Message::TransferConflictRenameInputChanged(value) => {
                self.update_transfer_conflict_rename(value);
                Command::none()
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
                Command::none()
            }
            Message::SelectAll => self.select_all_visible(),
        }
    }

    fn view(&self, window: window::Id) -> Element<'_, Self::Message> {
        if self.search_window == Some(window) {
            view_search_window(self.search.as_ref())
        } else if self.preview_window == Some(window) {
            view_preview_window(
                self.preview.as_ref(),
                self.preview_size,
                self.audio_preview.as_ref(),
            )
        } else {
            if !self.is_loading {
                startup_trace::mark_once("first_browser_view_after_initial_load");
            }
            view_browser(self)
        }
    }
}

impl FileBrowser {
    fn start_column_resize_drag(&mut self) -> Command<Message> {
        if self.renaming.is_some() {
            return self.commit_rename_if_active();
        }

        if self.column_view_mode != ColumnViewMode::Unbounded {
            return Command::none();
        }

        self.column_resize_drag = Some(ColumnResizeDrag {
            cursor_start_x: self.cursor_position.x,
            width_start: self.unbounded_column_width,
        });
        self.drag_selection_anchor = None;
        self.selection_marquee = None;
        self.clear_preview();
        self.context_menu = None;
        self.is_column_view_settings_open = false;
        Command::none()
    }

    fn update_column_resize_drag(&mut self, position: Point) {
        let Some(drag) = self.column_resize_drag else {
            return;
        };

        self.unbounded_column_width = config::normalize_unbounded_column_width(
            drag.width_start + position.x - drag.cursor_start_x,
        );
    }

    fn finish_column_resize_drag(&mut self) -> bool {
        let was_resizing = self.column_resize_drag.take().is_some();
        if was_resizing {
            self.user_config.unbounded_column_width = self.unbounded_column_width;
        }
        was_resizing
    }

    fn finish_column_resize_drag_command(&mut self) -> Command<Message> {
        if self.finish_column_resize_drag() {
            self.persist_user_config_command()
        } else {
            Command::none()
        }
    }

    fn persist_user_config_command(&self) -> Command<Message> {
        save_user_config_command(self.user_config.clone())
    }

    fn toggle_show_hidden_files(&mut self) -> Command<Message> {
        self.options.include_hidden = !self.options.include_hidden;
        self.user_config.show_hidden_files = self.options.include_hidden;
        let persist_command = self.persist_user_config_command();
        let reload_command = self.reload_current();
        Command::batch([persist_command, reload_command])
    }
}
