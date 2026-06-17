use std::path::PathBuf;
use std::time::Duration;

use file_core::watch_directory;
use iced::futures::SinkExt;
use iced::{Subscription, Task, Theme};

use super::events::system_theme;
use super::FileBrowser;
use crate::model::Message;
use crate::startup_trace;

const DIRECTORY_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
const DIRECTORY_WATCH_CHANNEL_SIZE: usize = 8;
const SIDEBAR_DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const OPERATION_QUEUE_AUTO_HIDE_DURATION: Duration = Duration::from_secs(5);
const SCROLLBAR_AUTO_HIDE_DURATION: Duration = Duration::from_millis(650);

pub(crate) fn run() -> iced::Result {
    iced::daemon(FileBrowser::boot, FileBrowser::update, FileBrowser::view)
        .subscription(FileBrowser::subscription)
        .theme(FileBrowser::theme)
        .title(FileBrowser::title)
        .run()
}

pub(super) fn directory_watch_subscription(path: PathBuf) -> Subscription<Message> {
    Subscription::run_with(path, directory_watch_stream)
}

pub(super) fn sidebar_device_refresh_subscription() -> Subscription<Message> {
    iced::time::every(SIDEBAR_DEVICE_REFRESH_INTERVAL)
        .map(|_| Message::SidebarDevicesRefreshRequested)
}

fn directory_watch_stream(path: &PathBuf) -> impl iced::futures::Stream<Item = Message> + 'static {
    let path = path.clone();
    iced::stream::channel(DIRECTORY_WATCH_CHANNEL_SIZE, async move |mut output| {
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
    })
}

pub(super) fn system_theme_command() -> Task<Message> {
    Task::perform(
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

pub(super) fn operation_queue_auto_hide_command(generation: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(OPERATION_QUEUE_AUTO_HIDE_DURATION).await;
            generation
        },
        Message::FileOperationAutoHideElapsed,
    )
}

pub(super) fn scrollbar_auto_hide_command(
    region: crate::model::ScrollbarRegion,
    generation: u64,
) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(SCROLLBAR_AUTO_HIDE_DURATION).await;
            (region, generation)
        },
        |(region, generation)| Message::ScrollbarAutoHideElapsed(region, generation),
    )
}
