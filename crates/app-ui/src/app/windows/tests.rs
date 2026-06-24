use std::collections::HashMap;
use std::fs;

use file_operation_store::TaskQueueStore;
use iced::futures::StreamExt;
use iced::Task;
use iced_runtime::Action;

use super::*;
use crate::config;
use crate::model::{BrowserViewMode, LoadedOperationStore, Message};

const FLOAT_TOLERANCE: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownAction {
    BrowserSessionSaved,
    Exit,
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

#[tokio::test(flavor = "multi_thread")]
async fn close_all_windows_saves_browser_session_before_exit() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let root = temp_dir.path().join("home");
    let deepest = root.join("project").join("src");
    fs::create_dir_all(&deepest).expect("create column directory chain");
    let store =
        TaskQueueStore::new(temp_dir.path().join("state.sqlite")).expect("create state store");

    let mut user_config = config::default_user_config();
    user_config.save_view_state = true;
    user_config.browser_view_mode = BrowserViewMode::Columns;
    let (mut browser, _) = FileBrowser::new(user_config);
    drop(browser.accept_operation_store(Ok(LoadedOperationStore {
        task_queue_store: store.clone(),
        column_width_overrides: HashMap::new(),
        browser_session: None,
        restored_tasks: Vec::new(),
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
