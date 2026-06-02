use std::path::PathBuf;
use std::time::Duration;

use file_core::watch_directory;
use iced::futures::SinkExt;
use iced::{Command, Subscription, Theme};

use super::events::system_theme;
use crate::model::Message;
use crate::startup_trace;

const DIRECTORY_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
const DIRECTORY_WATCH_CHANNEL_SIZE: usize = 8;
const OPERATION_QUEUE_AUTO_HIDE_DURATION: Duration = Duration::from_secs(5);
const SCROLLBAR_AUTO_HIDE_DURATION: Duration = Duration::from_millis(650);

pub(super) fn directory_watch_subscription(path: PathBuf) -> Subscription<Message> {
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

pub(super) fn system_theme_command() -> Command<Message> {
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

pub(super) fn operation_queue_auto_hide_command(generation: u64) -> Command<Message> {
    Command::perform(
        async move {
            tokio::time::sleep(OPERATION_QUEUE_AUTO_HIDE_DURATION).await;
            generation
        },
        Message::FileOperationAutoHideElapsed,
    )
}

pub(super) fn scrollbar_auto_hide_command(generation: u64) -> Command<Message> {
    Command::perform(
        async move {
            tokio::time::sleep(SCROLLBAR_AUTO_HIDE_DURATION).await;
            generation
        },
        Message::ScrollbarAutoHideElapsed,
    )
}
