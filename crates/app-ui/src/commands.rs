use std::ffi::OsString;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use desktop_linux::{
    open_path, read_desktop_clipboard, write_file_clipboard, FileClipboardSelection,
};
use file_core::{
    build_file_search_index, copy_path_with_controls_and_strategy, create_directory,
    create_empty_file, create_file_with_contents, delete_trash_entry, empty_trash,
    move_path_with_controls_and_strategy, rename_path, restore_trash_entry, scan_directory,
    scan_trash, search_file_index, search_file_tree, trash_path, CopyProgress, DirectoryScan,
    FileKind, FileOperationControls, FileSearchIndexOptions, FileSearchOptions, ScanOptions,
    TransferConflictStrategy, TrashRestoreEntry, TrashScan,
};
use file_operation_store::TaskQueueStore;
use iced::futures::channel::mpsc::Sender as IcedSender;
use iced::futures::SinkExt;
use iced::{Command, Subscription};

use crate::audio_preview::{start_audio_preview, start_audio_preview_at};
use crate::config;
use crate::model::{
    InitialLoad, Message, PendingOperation, SearchRequest, SidebarLocation, TransferConflictItem,
    TransferConflictMetadata, TransferConflictMode, TransferConflictState,
};
use crate::operation_queue::{
    FileOperationProgressUpdate, QueuedFileOperation, QueuedTransfer, RunningFileOperation,
    NEW_DIRECTORY_NAME, NEW_FILE_NAME,
};
use crate::preview::load_preview;
use crate::sidebar::{home_sidebar_location, sidebar_locations};
use crate::startup_trace;
use crate::thumbnail_cache::{ThumbnailLoadOutcome, ThumbnailWork};
use crate::video_preview::load_video_preview_frame;

const PATH_SUGGESTION_LIMIT: usize = 6;
const SEARCH_MATCH_LIMIT: usize = 50;
const FILE_OPERATION_CHANNEL_SIZE: usize = 32;
const COPY_PROGRESS_UI_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn initial_load_command() -> Command<Message> {
    Command::perform(load_initial_state(), Message::InitialLoadFinished)
}

pub(crate) fn save_user_config_command(user_config: config::UserConfig) -> Command<Message> {
    Command::perform(persist_user_config(user_config), Message::UserConfigSaved)
}

pub(crate) fn load_directory_command(path: PathBuf, options: ScanOptions) -> Command<Message> {
    Command::perform(load_directory(path, options), Message::Loaded)
}

pub(crate) fn load_trash_command(options: ScanOptions) -> Command<Message> {
    Command::perform(load_trash(options), Message::TrashLoaded)
}

pub(crate) fn load_expanded_directory_command(
    path: PathBuf,
    options: ScanOptions,
) -> Command<Message> {
    let expanded_path = path.clone();
    Command::perform(load_directory(path, options), move |scan| {
        Message::ExpandedDirectoryLoaded(expanded_path.clone(), scan)
    })
}

pub(crate) fn path_suggestions_command(input: String, current_dir: PathBuf) -> Command<Message> {
    let query = input.clone();
    Command::perform(
        load_path_suggestions(input, current_dir),
        move |suggestions| Message::PathSuggestionsLoaded(query, suggestions),
    )
}

pub(crate) fn search_command(
    request: SearchRequest,
    options: ScanOptions,
    index_dir: PathBuf,
) -> Command<Message> {
    let issued_request = request.clone();
    Command::perform(
        load_search_matches(request, options, index_dir),
        move |search| Message::SearchMatchesLoaded(issued_request.clone(), search),
    )
}

pub(crate) fn search_tree_command(
    request: SearchRequest,
    options: ScanOptions,
) -> Command<Message> {
    let issued_request = request.clone();
    Command::perform(load_search_tree_matches(request, options), move |search| {
        Message::SearchMatchesLoaded(issued_request.clone(), search)
    })
}

pub(crate) fn search_index_command(
    root: PathBuf,
    index_dir: PathBuf,
    options: ScanOptions,
) -> Command<Message> {
    let issued_root = root.clone();
    Command::perform(
        build_search_index(root, index_dir, options),
        move |outcome| Message::SearchIndexBuilt(issued_root.clone(), outcome),
    )
}

pub(crate) fn preview_command(
    path: PathBuf,
    kind: FileKind,
    options: ScanOptions,
) -> Command<Message> {
    let preview_path = path.clone();
    Command::perform(load_preview(path, kind, options), move |preview_outcome| {
        Message::PreviewLoaded(preview_path, preview_outcome)
    })
}

pub(crate) fn image_preview_dimensions_command(path: PathBuf) -> Command<Message> {
    let image_path = path.clone();
    Command::perform(load_image_dimensions(path), move |dimensions| {
        Message::ImagePreviewDimensionsLoaded(image_path.clone(), dimensions)
    })
}

pub(crate) fn start_audio_preview_command(path: PathBuf) -> Command<Message> {
    let audio_path = path.clone();
    Command::perform(start_audio_preview(path), move |playback_outcome| {
        Message::AudioPreviewStarted(audio_path.clone(), playback_outcome)
    })
}

pub(crate) fn start_video_preview_audio_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Command<Message> {
    let video_path = path.clone();
    Command::perform(
        start_audio_preview_at(path, position),
        move |audio_outcome| {
            Message::VideoPreviewAudioStarted(video_path.clone(), generation, audio_outcome)
        },
    )
}

pub(crate) fn video_preview_frame_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Command<Message> {
    let video_path = path.clone();
    Command::perform(
        load_video_preview_frame(path, generation, position),
        move |frame_outcome| match frame_outcome {
            Ok(frame) => Message::VideoPreviewFrameLoaded(frame),
            Err(error) => {
                Message::VideoPreviewSeekFrameFailed(video_path.clone(), generation, error)
            }
        },
    )
}

pub(crate) fn thumbnail_batch_command(
    cache_dir: PathBuf,
    works: Vec<ThumbnailWork>,
) -> Command<Message> {
    Command::perform(
        load_thumbnail_batch(cache_dir, works),
        Message::ThumbnailBatchLoaded,
    )
}

pub(crate) fn file_operation_subscription(task: RunningFileOperation) -> Subscription<Message> {
    let task_id = task.id;
    iced::subscription::channel(
        ("file-operation", task_id),
        FILE_OPERATION_CHANNEL_SIZE,
        move |mut output| async move {
            let result =
                run_queued_file_operation(task.operation, task.controls, task_id, &mut output)
                    .await;
            let _ = output
                .send(Message::FileOperationFinished(task_id, result))
                .await;
            iced::futures::future::pending().await
        },
    )
}

pub(crate) fn open_file_command(path: PathBuf) -> Command<Message> {
    Command::perform(
        async move { open_path(path).await.map_err(|error| error.to_string()) },
        Message::OpenFileFinished,
    )
}

pub(crate) fn write_file_clipboard_command(selection: FileClipboardSelection) -> Command<Message> {
    Command::perform(
        async move {
            write_file_clipboard(selection)
                .await
                .map_err(|error| error.to_string())
        },
        Message::FileClipboardWriteFinished,
    )
}

pub(crate) fn read_desktop_clipboard_command(
    paste_directory: PathBuf,
    fallback_operation: Option<PendingOperation>,
) -> Command<Message> {
    let issued_directory = paste_directory.clone();
    let issued_fallback = fallback_operation.clone();
    Command::perform(
        async move {
            read_desktop_clipboard()
                .await
                .map_err(|error| error.to_string())
        },
        move |content| Message::DesktopClipboardReadFinished {
            paste_directory: issued_directory.clone(),
            fallback_operation: issued_fallback.clone(),
            content,
        },
    )
}

pub(crate) fn create_clipboard_file_command(path: PathBuf, contents: Vec<u8>) -> Command<Message> {
    Command::perform(
        create_clipboard_file_at_available_path(path, contents),
        Message::ClipboardFileCreated,
    )
}

pub(crate) fn check_transfer_conflicts_command(
    mode: TransferConflictMode,
    transfers: Vec<QueuedTransfer>,
) -> Command<Message> {
    let issued_transfers = transfers.clone();
    Command::perform(check_transfer_conflicts(transfers), move |conflicts| {
        Message::TransferConflictsChecked {
            mode,
            transfers: issued_transfers.clone(),
            conflicts,
        }
    })
}

pub(crate) fn check_transfer_rename_target_command(
    state: TransferConflictState,
    transfer_position: Option<usize>,
    target: PathBuf,
) -> Command<Message> {
    let issued_state = state.clone();
    let issued_target = target.clone();
    Command::perform(
        async move { path_is_available(&target).await },
        move |available| Message::TransferConflictRenameTargetChecked {
            state: issued_state.clone(),
            transfer_position,
            target: issued_target.clone(),
            available,
        },
    )
}

async fn create_clipboard_file_at_available_path(
    path: PathBuf,
    contents: Vec<u8>,
) -> Result<PathBuf, String> {
    let target = available_alternate_path(&path).await?;
    create_file_with_contents(target, contents)
        .await
        .map_err(|error| error.to_string())
}

async fn check_transfer_conflicts(transfers: Vec<QueuedTransfer>) -> Vec<TransferConflictItem> {
    let mut conflicts = Vec::new();
    for transfer in transfers {
        if let Some(conflict) = transfer_conflict(transfer).await {
            conflicts.push(conflict);
        }
    }
    conflicts
}

async fn transfer_conflict(transfer: QueuedTransfer) -> Option<TransferConflictItem> {
    let source_metadata = metadata_if_exists(&transfer.source).await.ok().flatten()?;
    let target_metadata = metadata_if_exists(&transfer.target).await.ok().flatten()?;
    Some(TransferConflictItem {
        source: transfer.source,
        target: transfer.target,
        source_metadata: transfer_conflict_metadata(source_metadata),
        target_metadata: transfer_conflict_metadata(target_metadata),
    })
}

fn transfer_conflict_metadata(metadata: std::fs::Metadata) -> TransferConflictMetadata {
    TransferConflictMetadata {
        is_directory: metadata.is_dir(),
        len: metadata.len(),
        modified: metadata.modified().ok(),
    }
}

async fn path_is_available(path: &Path) -> Result<bool, String> {
    metadata_if_exists(path)
        .await
        .map(|metadata| metadata.is_none())
}

async fn available_alternate_path(path: &Path) -> Result<PathBuf, String> {
    if path_is_available(path).await? {
        return Ok(path.to_path_buf());
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let name = path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("item"));

    for index in 1..1000 {
        let mut next = name.clone();
        next.push(format!(".copy{index}"));
        let candidate = parent.join(next);
        if path_is_available(&candidate).await? {
            return Ok(candidate);
        }
    }

    Ok(path.to_path_buf())
}

async fn metadata_if_exists(path: &Path) -> Result<Option<std::fs::Metadata>, String> {
    match tokio::fs::metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read metadata for {path:?}: {error}")),
    }
}

async fn run_queued_file_operation(
    operation: QueuedFileOperation,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    match operation {
        QueuedFileOperation::Rename { path, new_name } => {
            run_queued_rename(path, new_name, controls, task_id, output).await
        }
        QueuedFileOperation::CreateDirectory { parent } => {
            run_queued_create_directory(parent, controls, task_id, output).await
        }
        QueuedFileOperation::CreateEmptyFile { parent } => {
            run_queued_create_empty_file(parent, controls, task_id, output).await
        }
        QueuedFileOperation::Trash { paths } => {
            run_queued_trash(paths, controls, task_id, output).await
        }
        QueuedFileOperation::Restore { entries } => {
            run_queued_restore(entries, controls, task_id, output).await
        }
        QueuedFileOperation::DeleteTrashEntries { entries } => {
            run_queued_delete_trash_entries(entries, controls, task_id, output).await
        }
        QueuedFileOperation::EmptyTrash => run_queued_empty_trash(controls, task_id, output).await,
        QueuedFileOperation::Copy { transfers } => {
            run_queued_transfers(
                transfers,
                controls,
                task_id,
                output,
                QueuedTransferMode::Copy,
            )
            .await
        }
        QueuedFileOperation::Move { transfers } => {
            run_queued_transfers(
                transfers,
                controls,
                task_id,
                output,
                QueuedTransferMode::Move,
            )
            .await
        }
    }
}

async fn run_queued_rename(
    path: PathBuf,
    new_name: String,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    send_file_operation_progress(output, task_id, FileOperationProgressUpdate::Indeterminate).await;
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    rename_path(path, OsString::from(new_name))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(())
}

async fn run_queued_create_directory(
    parent: PathBuf,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    create_directory(parent.join(NEW_DIRECTORY_NAME))
        .await
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(())
}

async fn run_queued_create_empty_file(
    parent: PathBuf,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    create_empty_file(parent.join(NEW_FILE_NAME))
        .await
        .map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(())
}

async fn run_queued_trash(
    paths: Vec<PathBuf>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    let total = paths.len();
    for (index, path) in paths.into_iter().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        trash_path(path).await.map_err(|error| error.to_string())?;
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(())
}

async fn run_queued_restore(
    entries: Vec<TrashRestoreEntry>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    let total = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        restore_trash_entry(entry, TransferConflictStrategy::KeepBoth)
            .await
            .map_err(|error| error.to_string())?;
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(())
}

async fn run_queued_delete_trash_entries(
    entries: Vec<TrashRestoreEntry>,
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    let total = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        controls
            .wait_until_running()
            .await
            .map_err(|error| error.to_string())?;
        delete_trash_entry(entry)
            .await
            .map_err(|error| error.to_string())?;
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(())
}

async fn run_queued_empty_trash(
    mut controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
) -> Result<(), String> {
    send_file_operation_progress(output, task_id, FileOperationProgressUpdate::Indeterminate).await;
    controls
        .wait_until_running()
        .await
        .map_err(|error| error.to_string())?;
    empty_trash().await.map_err(|error| error.to_string())?;
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Items {
            completed: 1,
            total: 1,
        },
    )
    .await;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum QueuedTransferMode {
    Copy,
    Move,
}

async fn run_queued_transfers(
    transfers: Vec<QueuedTransfer>,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
    mode: QueuedTransferMode,
) -> Result<(), String> {
    let total = transfers.len();
    for (index, transfer) in transfers.into_iter().enumerate() {
        run_queued_transfer(
            transfer,
            controls.clone(),
            task_id,
            output,
            mode,
            index,
            total,
        )
        .await?;
        send_file_operation_progress(
            output,
            task_id,
            FileOperationProgressUpdate::Items {
                completed: index + 1,
                total,
            },
        )
        .await;
    }
    Ok(())
}

async fn run_queued_transfer(
    transfer: QueuedTransfer,
    controls: FileOperationControls,
    task_id: u64,
    output: &mut IcedSender<Message>,
    mode: QueuedTransferMode,
    completed_transfers: usize,
    total_transfers: usize,
) -> Result<(), String> {
    let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::unbounded_channel();
    let transfer = async move {
        match mode {
            QueuedTransferMode::Copy => {
                copy_path_with_controls_and_strategy(
                    transfer.source,
                    transfer.target,
                    controls,
                    Some(progress_sender),
                    transfer.conflict_strategy,
                )
                .await
            }
            QueuedTransferMode::Move => {
                move_path_with_controls_and_strategy(
                    transfer.source,
                    transfer.target,
                    controls,
                    Some(progress_sender),
                    transfer.conflict_strategy,
                )
                .await
            }
        }
    };
    tokio::pin!(transfer);
    let mut latest_copy_progress = None;
    let mut last_copy_progress_sent_at = None;

    loop {
        tokio::select! {
            progress = progress_receiver.recv() => {
                if let Some(progress) = progress {
                    latest_copy_progress = Some(progress);
                    let now = Instant::now();
                    if should_send_copy_progress(last_copy_progress_sent_at, now) {
                        if let Some(progress) = latest_copy_progress.take() {
                            send_copy_progress(
                                output,
                                task_id,
                                progress,
                                completed_transfers,
                                total_transfers,
                            ).await;
                            last_copy_progress_sent_at = Some(now);
                        }
                    }
                }
            }
            transfer_outcome = &mut transfer => {
                if let Some(progress) = latest_copy_progress.take() {
                    send_copy_progress(
                        output,
                        task_id,
                        progress,
                        completed_transfers,
                        total_transfers,
                    ).await;
                }
                return transfer_outcome.map_err(|error| error.to_string());
            }
        }
    }
}

fn should_send_copy_progress(last_sent_at: Option<Instant>, now: Instant) -> bool {
    match last_sent_at {
        Some(last_sent_at) => now.duration_since(last_sent_at) >= COPY_PROGRESS_UI_INTERVAL,
        None => true,
    }
}

async fn send_copy_progress(
    output: &mut IcedSender<Message>,
    task_id: u64,
    progress: CopyProgress,
    completed_transfers: usize,
    total_transfers: usize,
) {
    send_file_operation_progress(
        output,
        task_id,
        FileOperationProgressUpdate::Bytes {
            bytes_done: progress.bytes_done,
            bytes_total: progress.bytes_total,
            completed_transfers,
            total_transfers,
        },
    )
    .await;
}

async fn send_file_operation_progress(
    output: &mut IcedSender<Message>,
    task_id: u64,
    progress: FileOperationProgressUpdate,
) {
    let _ = output
        .send(Message::FileOperationProgressed(task_id, progress))
        .await;
}

async fn load_directory(path: PathBuf, options: ScanOptions) -> Result<DirectoryScan, String> {
    startup_trace::mark_once("initial_directory_scan_started");
    let scan_outcome = scan_directory(path, options)
        .await
        .map_err(|error| error.to_string());
    startup_trace::mark_once("initial_directory_scan_finished");
    scan_outcome
}

async fn load_trash(options: ScanOptions) -> Result<TrashScan, String> {
    scan_trash(options).await.map_err(|error| error.to_string())
}

async fn load_initial_state() -> InitialLoad {
    startup_trace::mark_once("initial_load_started");
    let (home, user_config, state_database_path) = initial_paths().await;
    let mut options = ScanOptions::default();
    options.include_hidden = user_config.show_hidden_files;
    let scan = load_directory(home.clone(), options);
    let sidebar_locations = load_sidebar_locations(home.clone());
    let operation_store = load_operation_store(state_database_path);
    let (scan, sidebar_locations, operation_store) =
        tokio::join!(scan, sidebar_locations, operation_store);
    startup_trace::mark_once("initial_load_finished");
    InitialLoad {
        home,
        scan,
        sidebar_locations,
        user_config,
        operation_store,
    }
}

async fn initial_paths() -> (PathBuf, config::UserConfig, PathBuf) {
    tokio::task::spawn_blocking(|| {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let user_config = config::load_user_config();
        (home, user_config, config::default_state_database_path())
    })
    .await
    .unwrap_or_else(|_| {
        (
            PathBuf::from("/"),
            config::default_user_config(),
            config::default_state_database_path(),
        )
    })
}

async fn persist_user_config(user_config: config::UserConfig) -> Result<(), String> {
    tokio::task::spawn_blocking(move || config::save_user_config(&user_config))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

async fn load_operation_store(path: PathBuf) -> Result<TaskQueueStore, String> {
    let result = tokio::task::spawn_blocking(move || {
        let store = TaskQueueStore::new(path)?;
        store.clear_tasks()?;
        Ok::<TaskQueueStore, file_operation_store::StoreError>(store)
    })
    .await
    .map_err(|error| error.to_string())?;
    result.map_err(|error| error.to_string())
}

async fn load_thumbnail_batch(
    cache_dir: PathBuf,
    works: Vec<ThumbnailWork>,
) -> Vec<ThumbnailLoadOutcome> {
    let mut outcomes = Vec::with_capacity(works.len());
    for work in works {
        let result = thumbnails::load_or_generate_thumbnail(&cache_dir, work.request.clone())
            .await
            .map_err(|error| error.to_string());
        outcomes.push(ThumbnailLoadOutcome { work, result });
    }
    outcomes
}

async fn load_image_dimensions(path: PathBuf) -> Result<(u32, u32), String> {
    thumbnails::load_image_dimensions(path)
        .await
        .map_err(|error| error.to_string())
}

async fn load_sidebar_locations(home: PathBuf) -> Vec<SidebarLocation> {
    let fallback_home = home.clone();
    let locations = tokio::task::spawn_blocking(move || sidebar_locations(&home))
        .await
        .unwrap_or_else(|_| vec![home_sidebar_location(&fallback_home)]);
    startup_trace::mark_once("sidebar_locations_loaded");
    locations
}

async fn load_search_matches(
    request: SearchRequest,
    options: ScanOptions,
    index_dir: PathBuf,
) -> Result<file_core::FileSearchOutcome, String> {
    search_file_index(
        index_dir,
        request.root,
        request.query,
        FileSearchOptions {
            include_hidden: options.include_hidden,
            limit: SEARCH_MATCH_LIMIT,
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn load_search_tree_matches(
    request: SearchRequest,
    options: ScanOptions,
) -> Result<file_core::FileSearchOutcome, String> {
    search_file_tree(
        request.root,
        request.query,
        FileSearchOptions {
            include_hidden: options.include_hidden,
            limit: SEARCH_MATCH_LIMIT,
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn build_search_index(
    root: PathBuf,
    index_dir: PathBuf,
    options: ScanOptions,
) -> Result<file_core::FileSearchIndexOutcome, String> {
    build_file_search_index(
        root,
        index_dir,
        FileSearchIndexOptions {
            include_hidden: options.include_hidden,
        },
    )
    .await
    .map_err(|error| error.to_string())
}

async fn load_path_suggestions(input: String, current_dir: PathBuf) -> Vec<PathBuf> {
    let Some((directory, prefix)) = suggestion_directory_and_prefix(&input, &current_dir) else {
        return Vec::new();
    };
    let mut reader = match tokio::fs::read_dir(directory).await {
        Ok(reader) => reader,
        Err(_) => return Vec::new(),
    };

    let mut suggestions = Vec::new();
    while let Ok(Some(dir_entry)) = reader.next_entry().await {
        let file_name = dir_entry.file_name();
        if !file_name_starts_with(&file_name, &prefix) {
            continue;
        }

        let Ok(file_type) = dir_entry.file_type().await else {
            continue;
        };
        if file_type.is_dir() {
            suggestions.push(dir_entry.path());
        }
    }

    suggestions.sort_unstable();
    suggestions.truncate(PATH_SUGGESTION_LIMIT);
    suggestions
}

#[cfg(unix)]
fn file_name_starts_with(file_name: &std::ffi::OsStr, prefix: &str) -> bool {
    file_name.as_bytes().starts_with(prefix.as_bytes())
}

#[cfg(not(unix))]
fn file_name_starts_with(file_name: &std::ffi::OsStr, prefix: &str) -> bool {
    file_name.to_string_lossy().starts_with(prefix)
}

fn suggestion_directory_and_prefix(input: &str, current_dir: &Path) -> Option<(PathBuf, String)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let raw_path = PathBuf::from(trimmed);
    let path = if raw_path.is_absolute() {
        raw_path
    } else {
        current_dir.join(raw_path)
    };

    if trimmed.ends_with(std::path::MAIN_SEPARATOR) {
        return Some((path, String::new()));
    }

    let prefix = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current_dir.to_path_buf());

    Some((directory, prefix))
}
