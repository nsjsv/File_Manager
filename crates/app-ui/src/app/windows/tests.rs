use std::collections::HashMap;
use std::fs;

use file_operation_store::TaskQueueStore;
use iced::futures::StreamExt;
use iced::Task;
use iced_runtime::Action;

use super::*;
use crate::config;
use crate::model::{
    BrowserPaneId, BrowserPaneLayout, BrowserViewMode, LoadedOperationStore, Message, SplitAxis,
    WindowFrameState, WINDOW_TITLE_BAR_HEIGHT,
};
use crate::view::{main_pane_window_chrome_role, MainPaneWindowChromeRole};

const FLOAT_TOLERANCE: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownAction {
    BrowserSessionSaved,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedResizeDirection {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservedWindowAction {
    Minimize(window::Id),
    ToggleMaximize(window::Id),
    QueryMaximized(window::Id),
    Drag(window::Id),
    Resize(window::Id, ObservedResizeDirection),
    FrameObserved(window::Id, WindowFrameState),
}

async fn observed_window_actions(task: Task<Message>) -> Vec<ObservedWindowAction> {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    while let Some(action) = stream.next().await {
        match action {
            Action::Window(iced_runtime::window::Action::Minimize(window, true)) => {
                actions.push(ObservedWindowAction::Minimize(window));
            }
            Action::Window(iced_runtime::window::Action::ToggleMaximize(window)) => {
                actions.push(ObservedWindowAction::ToggleMaximize(window));
            }
            Action::Window(iced_runtime::window::Action::GetMaximized(window, sender)) => {
                actions.push(ObservedWindowAction::QueryMaximized(window));
                sender.send(false).expect("send maximized state");
            }
            Action::Window(iced_runtime::window::Action::Drag(window)) => {
                actions.push(ObservedWindowAction::Drag(window));
            }
            Action::Window(iced_runtime::window::Action::DragResize(window, direction)) => {
                let direction = match direction {
                    window::Direction::North => ObservedResizeDirection::North,
                    window::Direction::South => ObservedResizeDirection::South,
                    window::Direction::East => ObservedResizeDirection::East,
                    window::Direction::West => ObservedResizeDirection::West,
                    window::Direction::NorthEast => ObservedResizeDirection::NorthEast,
                    window::Direction::NorthWest => ObservedResizeDirection::NorthWest,
                    window::Direction::SouthEast => ObservedResizeDirection::SouthEast,
                    window::Direction::SouthWest => ObservedResizeDirection::SouthWest,
                };
                actions.push(ObservedWindowAction::Resize(window, direction));
            }
            Action::Output(Message::WindowMaximizedObserved(window, frame_state)) => {
                actions.push(ObservedWindowAction::FrameObserved(window, frame_state));
            }
            _ => {}
        }
    }
    actions
}

fn clamped_image_size(width: u32, height: u32) -> PreviewSize {
    clamp_preview_size_to_minimum(
        PreviewWindowProfile::Image,
        image_preview_size_from_dimensions(width, height),
    )
}

fn clamped_video_size(width: u32, height: u32) -> PreviewSize {
    clamp_preview_size_to_minimum(
        PreviewWindowProfile::Video,
        video_preview_size_from_frame(width, height),
    )
}

fn assert_close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= FLOAT_TOLERANCE,
        "expected {actual} to be within {FLOAT_TOLERANCE} of {expected}"
    );
}

async fn shutdown_actions(task: Task<Message>) -> Vec<ShutdownAction> {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    while let Some(action) = stream.next().await {
        match action {
            Action::Output(Message::BrowserSessionSaved(result)) => {
                assert_eq!(result, Ok(()));
                actions.push(ShutdownAction::BrowserSessionSaved);
            }
            Action::Exit => actions.push(ShutdownAction::Exit),
            _ => {}
        }
    }
    actions
}

#[test]
fn every_application_window_disables_native_decorations() {
    let main = main_window_settings();
    let settings = settings_window_settings();
    let properties = properties_window_settings();
    let preview = preview_window_settings(
        PreviewWindowProfile::Regular,
        default_preview_size(PreviewWindowProfile::Regular),
    );

    assert!(!main.decorations);
    assert!(!settings.decorations);
    assert!(!properties.decorations);
    assert!(!preview.decorations);
}

#[test]
fn auxiliary_windows_reserve_custom_title_bar_height_outside_content_size() {
    let settings = settings_window_settings();
    assert_close(
        settings.size.height,
        DEFAULT_SETTINGS_HEIGHT + WINDOW_TITLE_BAR_HEIGHT,
    );
    let properties = properties_window_settings();
    assert_close(
        properties.size.height,
        DEFAULT_PROPERTIES_HEIGHT + WINDOW_TITLE_BAR_HEIGHT,
    );

    let content_size = default_preview_size(PreviewWindowProfile::Regular);
    let preview = preview_window_settings(PreviewWindowProfile::Regular, content_size);
    assert_close(preview.size.width, content_size.width);
    assert_close(
        preview.size.height,
        content_size.height + WINDOW_TITLE_BAR_HEIGHT,
    );
}

#[test]
fn maximized_observation_is_isolated_and_removed_when_window_closes() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let settings_window = window::Id::unique();
    browser.settings_window = Some(settings_window);

    drop(browser.accept_window_maximized_observation(settings_window, WindowFrameState::Maximized));
    assert!(browser.maximized_windows.contains(&settings_window));
    assert!(!browser.maximized_windows.contains(&browser.main_window));

    drop(browser.close_settings_window());
    assert!(!browser.maximized_windows.contains(&settings_window));

    drop(browser.accept_window_maximized_observation(settings_window, WindowFrameState::Maximized));
    assert!(!browser.maximized_windows.contains(&settings_window));
}

#[tokio::test]
async fn window_control_messages_target_the_source_window() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let source_window = browser.main_window;

    let minimize =
        observed_window_actions(browser.update(Message::WindowMinimizeRequested(source_window)))
            .await;
    assert_eq!(
        minimize,
        vec![ObservedWindowAction::Minimize(source_window)]
    );

    let maximize =
        observed_window_actions(browser.update(Message::WindowMaximizeToggled(source_window)))
            .await;
    assert_eq!(
        maximize,
        vec![
            ObservedWindowAction::ToggleMaximize(source_window),
            ObservedWindowAction::QueryMaximized(source_window),
            ObservedWindowAction::FrameObserved(source_window, WindowFrameState::Restored),
        ]
    );

    let drag =
        observed_window_actions(browser.update(Message::WindowDragRequested(source_window))).await;
    assert_eq!(drag, vec![ObservedWindowAction::Drag(source_window)]);

    let resize_directions = [
        (window::Direction::North, ObservedResizeDirection::North),
        (window::Direction::South, ObservedResizeDirection::South),
        (window::Direction::East, ObservedResizeDirection::East),
        (window::Direction::West, ObservedResizeDirection::West),
        (
            window::Direction::NorthEast,
            ObservedResizeDirection::NorthEast,
        ),
        (
            window::Direction::NorthWest,
            ObservedResizeDirection::NorthWest,
        ),
        (
            window::Direction::SouthEast,
            ObservedResizeDirection::SouthEast,
        ),
        (
            window::Direction::SouthWest,
            ObservedResizeDirection::SouthWest,
        ),
    ];
    for (direction, observed_direction) in resize_directions {
        let resize = observed_window_actions(
            browser.update(Message::WindowResizeRequested(source_window, direction)),
        )
        .await;
        assert_eq!(
            resize,
            vec![ObservedWindowAction::Resize(
                source_window,
                observed_direction,
            )]
        );
    }
}

#[tokio::test]
async fn maximized_window_does_not_start_edge_resize() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let source_window = browser.main_window;
    drop(browser.accept_window_maximized_observation(source_window, WindowFrameState::Maximized));

    let actions = observed_window_actions(browser.update(Message::WindowResizeRequested(
        source_window,
        window::Direction::North,
    )))
    .await;

    assert!(actions.is_empty());
}

#[test]
fn pane_layout_assigns_one_outer_window_chrome_set() {
    let first = BrowserPaneId(1);
    let second = BrowserPaneId(2);

    assert_eq!(
        main_pane_window_chrome_role(BrowserPaneLayout::Single { active: first }, first),
        MainPaneWindowChromeRole::Complete
    );

    let horizontal = BrowserPaneLayout::Split {
        axis: SplitAxis::Horizontal,
        first,
        second,
        active: first,
    };
    assert_eq!(
        main_pane_window_chrome_role(horizontal, first),
        MainPaneWindowChromeRole::LeftControls
    );
    assert_eq!(
        main_pane_window_chrome_role(horizontal, second),
        MainPaneWindowChromeRole::RightControls
    );

    let vertical = BrowserPaneLayout::Split {
        axis: SplitAxis::Vertical,
        first,
        second,
        active: second,
    };
    assert_eq!(
        main_pane_window_chrome_role(vertical, first),
        MainPaneWindowChromeRole::Complete
    );
    assert_eq!(
        main_pane_window_chrome_role(vertical, second),
        MainPaneWindowChromeRole::NoChrome
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn close_all_windows_saves_browser_session_before_exit() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let root = temp_dir.path().join("home");
    let deepest = root.join("project").join("src");
    fs::create_dir_all(&deepest).expect("create column directory chain");
    let store =
        TaskQueueStore::new(temp_dir.path().join("state.sqlite")).expect("create state store");

    let mut user_config = config::default_user_config();
    user_config.startup_location_policy = config::StartupLocationPolicy::PreviousSession;
    user_config.save_view_state = user_config.startup_location_policy.saves_view_state();
    user_config.browser_view_mode = BrowserViewMode::Columns;
    let (mut browser, _) = FileBrowser::new(user_config);
    drop(browser.accept_operation_store(Ok(LoadedOperationStore {
        task_queue_store: store.clone(),
        column_width_overrides: HashMap::new(),
        classified_startup_session: None,
    })));

    browser.current_dir = root.clone();
    browser.view_mode = BrowserViewMode::Columns;
    browser.deepest_open_column_directory = Some(deepest.clone());
    browser.sync_active_tab_state();

    let actions = shutdown_actions(browser.close_auxiliary_window(browser.main_window)).await;
    let saved_position = actions
        .iter()
        .position(|action| *action == ShutdownAction::BrowserSessionSaved)
        .expect("shutdown should flush browser session");
    let exit_position = actions
        .iter()
        .position(|action| *action == ShutdownAction::Exit)
        .expect("shutdown should exit iced runtime");

    assert!(saved_position < exit_position);

    let stored_session = store
        .read_browser_session()
        .expect("read browser session")
        .expect("browser session should be stored");
    let stored_deepest_directory = stored_session.panes[0].tabs[0]
        .deepest_open_column_directory
        .as_ref()
        .map(|path| path.to_path_buf());
    assert_eq!(stored_deepest_directory.as_deref(), Some(deepest.as_path()));
}

#[test]
fn system_window_focus_ignores_late_unfocus_from_an_older_window() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let main_window = browser.main_window;
    let settings_window = window::Id::unique();

    assert_eq!(browser.system_focused_window, None);
    drop(browser.handle_window_focused(main_window));
    assert_eq!(browser.system_focused_window, Some(main_window));

    drop(browser.handle_window_focused(settings_window));
    drop(browser.handle_window_unfocused(main_window));
    assert_eq!(browser.system_focused_window, Some(settings_window));
    assert_eq!(browser.focused_window, settings_window);

    drop(browser.handle_window_unfocused(settings_window));
    assert_eq!(browser.system_focused_window, None);
}

#[test]
fn closing_focused_auxiliary_window_clears_system_focus() {
    let (mut browser, _) = FileBrowser::new(config::default_user_config());
    let settings_window = window::Id::unique();
    browser.settings_window = Some(settings_window);
    browser.focused_window = settings_window;
    browser.system_focused_window = Some(settings_window);

    drop(browser.close_settings_window());

    assert_eq!(browser.system_focused_window, None);
    assert_eq!(browser.focused_window, browser.main_window);
}

#[test]
fn image_preview_size_fits_large_landscape_to_max_width() {
    let size = clamped_image_size(3_000, 1_000);
    let max_size = image_preview_initial_fit_max_size();

    assert_close(size.width, max_size.width);
    assert!(size.height < max_size.height);
    assert_close(size.width / size.height, 3.0);
}

#[test]
fn image_preview_size_fits_large_portrait_to_max_height() {
    let size = clamped_image_size(1_000, 2_000);
    let max_size = image_preview_initial_fit_max_size();

    assert!(size.width < max_size.width);
    assert_close(size.height, max_size.height);
    assert_close(size.height / size.width, 2.0);
}

#[test]
fn image_preview_size_clamps_small_images_to_minimum_window() {
    let size = clamped_image_size(64, 32);
    let min_size = preview_min_size(PreviewWindowProfile::Image);

    assert_close(size.width, min_size.width);
    assert_close(size.height, min_size.height);
}

#[test]
fn image_preview_size_keeps_medium_landscape_tight() {
    let size = clamped_image_size(748, 499);

    assert_close(size.width, 748.0);
    assert_close(size.height, 499.0);
}

#[test]
fn image_preview_size_keeps_medium_portrait_tight() {
    let size = clamped_image_size(400, 600);

    assert_close(size.width, 400.0);
    assert_close(size.height, 600.0);
}

#[test]
fn video_preview_size_keeps_medium_frame_plus_controls_tight() {
    let size = clamped_video_size(640, 360);

    assert_close(size.width, 640.0);
    assert_close(size.height, 360.0 + VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT);
}

#[test]
fn video_preview_size_fits_large_portrait_frame_to_max_height() {
    let size = clamped_video_size(720, 1280);
    let max_size = video_preview_initial_fit_max_size();
    let frame_height = size.height - VIDEO_PREVIEW_WINDOW_CONTROL_HEIGHT;

    assert!(size.width < max_size.width);
    assert_close(size.height, max_size.height);
    assert_close(frame_height / size.width, 1280.0 / 720.0);
}
