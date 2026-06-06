use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use desktop_linux::{
    open_path_with_terminal_emulator, open_terminal_at_directory, read_desktop_clipboard,
    write_file_clipboard, FileClipboardSelection, TerminalEmulator,
};
use file_core::{
    available_transfer_target_path, build_file_search_index,
    check_transfer_conflicts as check_core_transfer_conflicts, create_file_with_contents,
    is_transfer_target_available, scan_directory, scan_trash, search_file_index, search_file_tree,
    DirectoryScan, FileKind, FileSearchIndexOptions, FileSearchOptions, ScanOptions,
    TransferConflictCheck, TransferConflictItem, TrashScan,
};
use file_operation_store::TaskQueueStore;
use iced::Task;

use crate::audio_preview::{start_audio_preview, start_audio_preview_at};
use crate::config;
use crate::model::{
    InitialLoad, Message, PathSuggestionRequest, PendingOperation, SearchRequest, SidebarLocation,
    TransferConflictMode, TransferConflictState,
};
use crate::operation_queue::QueuedTransfer;
use crate::preview::{load_directory_preview_children, load_preview};
use crate::sidebar::{home_sidebar_location, save_gtk_bookmark_locations, sidebar_locations};
use crate::startup_trace;
use crate::thumbnail_cache::{ThumbnailLoadOutcome, ThumbnailWork};
use crate::video_preview::{inspect_video_preview_metadata, load_video_preview_frame};

mod queued_file_operations;
pub(crate) use queued_file_operations::file_operation_subscription;

const PATH_SUGGESTION_LIMIT: usize = 6;
const SEARCH_MATCH_LIMIT: usize = 50;

pub(crate) fn initial_load_command(user_config: config::UserConfig) -> Task<Message> {
    Task::perform(
        load_initial_state(user_config),
        Message::InitialLoadFinished,
    )
}

pub(crate) fn save_user_config_command(user_config: config::UserConfig) -> Task<Message> {
    Task::perform(persist_user_config(user_config), Message::UserConfigSaved)
}

pub(crate) fn save_sidebar_bookmarks_command(bookmarks: Vec<SidebarLocation>) -> Task<Message> {
    Task::perform(
        persist_sidebar_bookmarks(bookmarks),
        Message::SidebarBookmarksSaved,
    )
}

pub(crate) fn load_directory_command(path: PathBuf, options: ScanOptions) -> Task<Message> {
    Task::perform(load_directory(path, options), Message::Loaded)
}

pub(crate) fn load_trash_command(options: ScanOptions) -> Task<Message> {
    Task::perform(load_trash(options), Message::TrashLoaded)
}

pub(crate) fn load_expanded_directory_command(
    path: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    let expanded_path = path.clone();
    Task::perform(load_directory(path, options), move |scan| {
        Message::ExpandedDirectoryLoaded(expanded_path.clone(), scan)
    })
}

pub(crate) fn path_suggestions_command(request: PathSuggestionRequest) -> Task<Message> {
    let issued_request = request.clone();
    Task::perform(
        load_path_suggestions(request.input, request.current_dir),
        move |suggestions| Message::PathSuggestionsLoaded(issued_request.clone(), suggestions),
    )
}

pub(crate) fn search_command(
    request: SearchRequest,
    options: ScanOptions,
    index_dir: PathBuf,
) -> Task<Message> {
    let issued_request = request.clone();
    Task::perform(
        load_search_matches(request, options, index_dir),
        move |search| Message::SearchMatchesLoaded(issued_request.clone(), search),
    )
}

pub(crate) fn search_tree_command(request: SearchRequest, options: ScanOptions) -> Task<Message> {
    let issued_request = request.clone();
    Task::perform(load_search_tree_matches(request, options), move |search| {
        Message::SearchMatchesLoaded(issued_request.clone(), search)
    })
}

pub(crate) fn search_index_command(
    root: PathBuf,
    index_dir: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    let issued_root = root.clone();
    Task::perform(
        build_search_index(root, index_dir, options),
        move |outcome| Message::SearchIndexBuilt(issued_root.clone(), outcome),
    )
}

pub(crate) fn preview_command(
    path: PathBuf,
    kind: FileKind,
    options: ScanOptions,
) -> Task<Message> {
    let preview_path = path.clone();
    Task::perform(load_preview(path, kind, options), move |preview_outcome| {
        Message::PreviewLoaded(preview_path.clone(), preview_outcome)
    })
}

pub(crate) fn preview_directory_children_command(
    path: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    let parent_path = path.clone();
    Task::perform(
        load_directory_preview_children(path, options),
        move |children_outcome| {
            Message::PreviewDirectoryChildrenLoaded(parent_path.clone(), children_outcome)
        },
    )
}

pub(crate) fn image_preview_dimensions_command(path: PathBuf) -> Task<Message> {
    let image_path = path.clone();
    Task::perform(load_image_dimensions(path), move |dimensions| {
        Message::ImagePreviewDimensionsLoaded(image_path.clone(), dimensions)
    })
}

pub(crate) fn start_audio_preview_command(path: PathBuf) -> Task<Message> {
    let audio_path = path.clone();
    Task::perform(start_audio_preview(path), move |playback_outcome| {
        Message::AudioPreviewStarted(audio_path.clone(), playback_outcome)
    })
}

pub(crate) fn start_video_preview_audio_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        start_audio_preview_at(path, position),
        move |audio_outcome| {
            Message::VideoPreviewAudioStarted(video_path.clone(), generation, audio_outcome)
        },
    )
}

pub(crate) fn video_preview_metadata_command(path: PathBuf) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        async move {
            inspect_video_preview_metadata(path)
                .await
                .map(|metadata| metadata.duration)
        },
        move |metadata_outcome| {
            Message::VideoPreviewMetadataLoaded(video_path.clone(), metadata_outcome)
        },
    )
}

pub(crate) fn video_preview_frame_command(
    path: PathBuf,
    generation: u64,
    position: Duration,
) -> Task<Message> {
    let video_path = path.clone();
    Task::perform(
        load_video_preview_frame(path, generation, position),
        move |frame_outcome| match frame_outcome {
            Ok(frame) => Message::VideoPreviewFrameLoaded(frame),
            Err(error) => Message::VideoPreviewSeekFrameFailed(
                video_path.clone(),
                generation,
                position,
                error,
            ),
        },
    )
}

pub(crate) fn thumbnail_batch_command(
    cache_dir: PathBuf,
    works: Vec<ThumbnailWork>,
) -> Task<Message> {
    Task::perform(
        load_thumbnail_batch(cache_dir, works),
        Message::ThumbnailBatchLoaded,
    )
}

pub(crate) fn open_file_command(
    path: PathBuf,
    terminal_emulator: TerminalEmulator,
) -> Task<Message> {
    Task::perform(
        async move {
            open_path_with_terminal_emulator(path, terminal_emulator)
                .await
                .map_err(|error| error.to_string())
        },
        Message::OpenFileFinished,
    )
}

pub(crate) fn open_terminal_command(
    directory: PathBuf,
    terminal_emulator: TerminalEmulator,
) -> Task<Message> {
    Task::perform(
        async move {
            open_terminal_at_directory(directory, terminal_emulator)
                .await
                .map_err(|error| error.to_string())
        },
        Message::OpenTerminalFinished,
    )
}

pub(crate) fn write_file_clipboard_command(selection: FileClipboardSelection) -> Task<Message> {
    Task::perform(
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
) -> Task<Message> {
    let issued_directory = paste_directory.clone();
    let issued_fallback = fallback_operation.clone();
    Task::perform(
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

pub(crate) fn create_clipboard_file_command(path: PathBuf, contents: Vec<u8>) -> Task<Message> {
    Task::perform(
        create_clipboard_file_at_available_path(path, contents),
        Message::ClipboardFileCreated,
    )
}

pub(crate) fn check_transfer_conflicts_command(
    mode: TransferConflictMode,
    transfers: Vec<QueuedTransfer>,
) -> Task<Message> {
    let issued_transfers = transfers.clone();
    Task::perform(check_transfer_conflicts(transfers), move |conflicts| {
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
) -> Task<Message> {
    let issued_state = state.clone();
    let issued_target = target.clone();
    Task::perform(
        async move {
            is_transfer_target_available(target)
                .await
                .map_err(|error| error.to_string())
        },
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
    let target = available_transfer_target_path(path)
        .await
        .map_err(|error| error.to_string())?;
    create_file_with_contents(target, contents)
        .await
        .map_err(|error| error.to_string())
}

async fn check_transfer_conflicts(transfers: Vec<QueuedTransfer>) -> Vec<TransferConflictItem> {
    let conflict_checks = transfers
        .into_iter()
        .map(|transfer| TransferConflictCheck::new(transfer.source, transfer.target))
        .collect();
    check_core_transfer_conflicts(conflict_checks).await
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

async fn load_initial_state(user_config: config::UserConfig) -> InitialLoad {
    startup_trace::mark_once("initial_load_started");
    let (home, user_config, state_database_path) = initial_paths(user_config).await;
    let options = ScanOptions {
        include_hidden: user_config.show_hidden_files,
        ..ScanOptions::default()
    };
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

async fn initial_paths(user_config: config::UserConfig) -> (PathBuf, config::UserConfig, PathBuf) {
    let fallback_config = user_config.clone();
    tokio::task::spawn_blocking(move || {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        (home, user_config, config::default_state_database_path())
    })
    .await
    .unwrap_or_else(|_| {
        (
            PathBuf::from("/"),
            fallback_config,
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

async fn persist_sidebar_bookmarks(bookmarks: Vec<SidebarLocation>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let home = dirs::home_dir().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable")
        })?;
        save_gtk_bookmark_locations(&home, &bookmarks)
    })
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
    let mut handles = Vec::with_capacity(works.len());
    for work in works {
        let task_cache_dir = cache_dir.clone();
        let fallback_work = work.clone();
        let handle = tokio::spawn(async move {
            let outcome =
                thumbnails::load_or_generate_thumbnail(task_cache_dir, work.request.clone())
                    .await
                    .map_err(|error| error.to_string());
            ThumbnailLoadOutcome {
                work,
                result: outcome,
            }
        });
        handles.push((fallback_work, handle));
    }

    let mut outcomes = Vec::with_capacity(handles.len());
    for (fallback_work, handle) in handles {
        match handle.await {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => outcomes.push(ThumbnailLoadOutcome {
                work: fallback_work,
                result: Err(format!("thumbnail task failed: {error}")),
            }),
        }
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
