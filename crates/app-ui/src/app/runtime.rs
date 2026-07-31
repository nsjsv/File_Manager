use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use desktop_linux::{WaylandDndController, WaylandDndEvent, WaylandDndWindowHandle};
use file_core::watch_directory;
use iced::futures::SinkExt;
use iced::{Subscription, Task, Theme};

use super::events::system_theme;
use super::FileBrowser;
use crate::command_line::ApplicationLaunchRequest;
use crate::model::Message;
use crate::startup_trace;

const DIRECTORY_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
const DIRECTORY_WATCH_CHANNEL_SIZE: usize = 8;
const SIDEBAR_DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const OPERATION_QUEUE_AUTO_HIDE_DURATION: Duration = Duration::from_secs(5);
const SCROLLBAR_AUTO_HIDE_DURATION: Duration = Duration::from_millis(650);

pub(crate) fn run(application_launch_request: ApplicationLaunchRequest) -> iced::Result {
    iced::daemon(
        move || FileBrowser::boot(application_launch_request.clone()),
        FileBrowser::update,
        FileBrowser::view,
    )
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

pub(super) fn wayland_file_dnd_subscription(
    window_handle: WaylandDndWindowHandle,
    controller: Arc<WaylandDndController>,
) -> Subscription<Message> {
    Subscription::run_with(
        WaylandDndSubscriptionState {
            window_handle,
            controller,
        },
        wayland_file_dnd_stream,
    )
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

#[derive(Debug, Clone)]
struct WaylandDndSubscriptionState {
    window_handle: WaylandDndWindowHandle,
    controller: Arc<WaylandDndController>,
}

impl PartialEq for WaylandDndSubscriptionState {
    fn eq(&self, other: &Self) -> bool {
        self.window_handle == other.window_handle && self.controller.id() == other.controller.id()
    }
}

impl Eq for WaylandDndSubscriptionState {}

impl std::hash::Hash for WaylandDndSubscriptionState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.window_handle.hash(state);
        self.controller.id().hash(state);
    }
}

fn wayland_file_dnd_stream(
    state: &WaylandDndSubscriptionState,
) -> impl iced::futures::Stream<Item = Message> + 'static {
    let state = state.clone();
    iced::stream::channel(16, async move |mut output| {
        let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_receiver) = mpsc::channel();
        let _shutdown_guard = WaylandDndShutdown::new(shutdown_sender);

        if let Err(error) = desktop_linux::spawn_wayland_file_dnd(
            state.window_handle,
            state.controller,
            event_sender,
            shutdown_receiver,
        ) {
            let _ = output
                .send(Message::WaylandFilesDropped(Err(error.to_string())))
                .await;
            return;
        }

        while let Some(event) = event_receiver.recv().await {
            let message = match event {
                WaylandDndEvent::FilesDropped(drop) => Message::WaylandFilesDropped(Ok(drop)),
                WaylandDndEvent::FileDropFailed(error) => Message::WaylandFilesDropped(Err(error)),
                WaylandDndEvent::FileDragSource(event) => {
                    Message::WaylandFileDragSourceEvent(event)
                }
                WaylandDndEvent::FileDragSelfTarget(event) => {
                    Message::WaylandFileDragSelfTargetEvent(event)
                }
                WaylandDndEvent::RuntimeFailed(error) => Message::WaylandDndRuntimeFailed(error),
            };
            if output.send(message).await.is_err() {
                break;
            }
        }
    })
}

struct WaylandDndShutdown {
    sender: Option<mpsc::Sender<()>>,
}

impl WaylandDndShutdown {
    fn new(sender: mpsc::Sender<()>) -> Self {
        Self {
            sender: Some(sender),
        }
    }
}

impl Drop for WaylandDndShutdown {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
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

pub(super) fn scrollbar_auto_hide_command(generation: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(SCROLLBAR_AUTO_HIDE_DURATION).await;
            generation
        },
        Message::ScrollbarAutoHideElapsed,
    )
}
