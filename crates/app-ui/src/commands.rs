use std::collections::HashMap;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
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
    BrowserPaneId, FilePropertiesDirectoryContents, FilePropertiesPermissions,
    FilePropertiesSnapshot, LoadedOperationStore, Message, PathSuggestionRequest, PendingOperation,
    SearchRequest, SidebarLocation, StartupEnvironment, TransferConflictMode,
    TransferConflictState,
};
use crate::operation_queue::QueuedTransfer;
use crate::preview::{load_directory_preview_children, load_preview};
use crate::sidebar::{home_sidebar_location, save_gtk_bookmark_locations, sidebar_locations};
use crate::startup_trace;
use crate::thumbnail_cache::{ThumbnailLoadOutcome, ThumbnailWork};
use crate::video_preview::{inspect_video_preview_metadata, load_video_preview_frame};

mod queued_file_operations;
pub(crate) use queued_file_operations::file_operation_subscription;
mod sidebar_devices;
pub(crate) use sidebar_devices::{sidebar_device_action_command, sidebar_devices_command};

const PATH_SUGGESTION_LIMIT: usize = 6;
const SEARCH_MATCH_LIMIT: usize = 50;
const THUMBNAIL_REFRESH_DELAY: Duration = Duration::from_millis(400);

pub(crate) fn startup_environment_command() -> Task<Message> {
    Task::perform(
        load_startup_environment(),
        Message::StartupEnvironmentLoaded,
    )
}

pub(crate) fn sidebar_locations_command(
    home: PathBuf,
    configured_favorites: Option<Vec<config::SidebarFavoriteConfig>>,
) -> Task<Message> {
    Task::perform(
        load_sidebar_locations(home, configured_favorites),
        Message::SidebarLocationsLoaded,
    )
}

pub(crate) fn operation_store_command(path: PathBuf) -> Task<Message> {
    Task::perform(load_operation_store(path), Message::OperationStoreLoaded)
}

pub(crate) fn delayed_thumbnail_refresh_command(
    pane_id: BrowserPaneId,
    directory: PathBuf,
) -> Task<Message> {
    Task::perform(delayed_thumbnail_refresh(directory), move |directory| {
        Message::ThumbnailRefreshRequested(pane_id, directory)
    })
}

pub(crate) fn save_user_config_command(user_config: config::UserConfig) -> Task<Message> {
    Task::perform(persist_user_config(user_config), Message::UserConfigSaved)
}

pub(crate) fn save_column_width_overrides_command(
    task_queue_store: TaskQueueStore,
    column_width_overrides: HashMap<usize, f32>,
) -> Task<Message> {
    Task::perform(
        persist_column_width_overrides(task_queue_store, column_width_overrides),
        Message::ColumnWidthOverrideSaved,
    )
}

pub(crate) fn save_sidebar_bookmarks_command(bookmarks: Vec<SidebarLocation>) -> Task<Message> {
    Task::perform(
        persist_sidebar_bookmarks(bookmarks),
        Message::SidebarBookmarksSaved,
    )
}

pub(crate) fn load_directory_command(
    pane_id: BrowserPaneId,
    path: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    Task::perform(load_directory(path, options), move |scan| {
        Message::Loaded(pane_id, scan)
    })
}

pub(crate) fn load_trash_command(pane_id: BrowserPaneId, options: ScanOptions) -> Task<Message> {
    Task::perform(load_trash(options), move |scan| {
        Message::TrashLoaded(pane_id, scan)
    })
}

pub(crate) fn load_expanded_directory_command(
    pane_id: BrowserPaneId,
    path: PathBuf,
    options: ScanOptions,
) -> Task<Message> {
    let expanded_path = path.clone();
    Task::perform(load_directory(path, options), move |scan| {
        Message::ExpandedDirectoryLoaded(pane_id, expanded_path.clone(), scan)
    })
}

pub(crate) fn path_suggestions_command(
    pane_id: BrowserPaneId,
    request: PathSuggestionRequest,
) -> Task<Message> {
    let issued_request = request.clone();
    Task::perform(
        load_path_suggestions(request.input, request.current_dir),
        move |suggestions| {
            Message::PathSuggestionsLoaded(pane_id, issued_request.clone(), suggestions)
        },
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

pub(crate) fn file_properties_command(path: PathBuf) -> Task<Message> {
    let requested_path = path.clone();
    Task::perform(load_file_properties(path), move |properties_outcome| {
        Message::FilePropertiesLoaded(requested_path.clone(), properties_outcome)
    })
}

pub(crate) fn set_file_properties_permissions_command(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Task<Message> {
    let requested_path = path.clone();
    Task::perform(
        set_file_properties_permissions(path, permissions),
        move |permissions_outcome| {
            Message::FilePropertiesPermissionsUpdated(requested_path.clone(), permissions_outcome)
        },
    )
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

pub(crate) fn startup_index_directory_children_command(
    path: PathBuf,
    request_generation: u64,
    options: ScanOptions,
) -> Task<Message> {
    let parent_path = path.clone();
    Task::perform(
        load_directory_preview_children(path, options),
        move |children_outcome| {
            Message::StartupIndexDirectoryChildrenLoaded(
                request_generation,
                parent_path.clone(),
                children_outcome,
            )
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

async fn load_file_properties(path: PathBuf) -> Result<FilePropertiesSnapshot, String> {
    tokio::task::spawn_blocking(move || read_file_properties(path))
        .await
        .map_err(|error| error.to_string())?
}

async fn set_file_properties_permissions(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    tokio::task::spawn_blocking(move || write_file_properties_permissions(path, permissions))
        .await
        .map_err(|error| error.to_string())?
}

fn read_file_properties(path: PathBuf) -> Result<FilePropertiesSnapshot, String> {
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else if file_type.is_symlink() {
        FileKind::Symlink
    } else {
        FileKind::Other
    };
    let type_label = if file_type.is_symlink() {
        "Symbolic Link".to_owned()
    } else if file_type.is_dir() {
        "Folder".to_owned()
    } else if file_type.is_file() {
        "File".to_owned()
    } else {
        "Other".to_owned()
    };
    let name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| path.as_os_str().to_os_string());
    let location = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));

    let mut directory_contents = None;
    let mut directory_contents_error = None;
    if file_type.is_dir() {
        match read_directory_properties_contents(&path) {
            Ok(contents) => directory_contents = Some(contents),
            Err(error) => directory_contents_error = Some(error),
        }
    }

    let size_bytes = directory_contents
        .as_ref()
        .map(|contents| contents.total_size_bytes)
        .unwrap_or_else(|| metadata.len());
    let disk_size_bytes = directory_contents
        .as_ref()
        .map(|contents| contents.total_disk_size_bytes)
        .unwrap_or_else(|| metadata_disk_size(&metadata));

    Ok(FilePropertiesSnapshot {
        name,
        kind,
        type_label,
        location,
        created: metadata.created().ok(),
        modified: metadata.modified().ok(),
        accessed: metadata.accessed().ok(),
        size_bytes,
        disk_size_bytes,
        directory_contents,
        directory_contents_error,
        permissions: metadata_properties_permissions(&metadata, file_type.is_symlink()),
    })
}

#[cfg(unix)]
fn metadata_properties_permissions(
    metadata: &std::fs::Metadata,
    is_symlink: bool,
) -> Option<FilePropertiesPermissions> {
    (!is_symlink).then(|| FilePropertiesPermissions::from_mode(metadata.permissions().mode()))
}

#[cfg(not(unix))]
fn metadata_properties_permissions(
    _metadata: &std::fs::Metadata,
    _is_symlink: bool,
) -> Option<FilePropertiesPermissions> {
    None
}

#[cfg(unix)]
fn write_file_properties_permissions(
    path: PathBuf,
    permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("symbolic link permissions cannot be changed".to_owned());
    }

    let mut fs_permissions = metadata.permissions();
    fs_permissions.set_mode(permissions.mode());
    std::fs::set_permissions(&path, fs_permissions).map_err(|error| error.to_string())?;

    let refreshed = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    Ok(FilePropertiesPermissions::from_mode(
        refreshed.permissions().mode(),
    ))
}

#[cfg(not(unix))]
fn write_file_properties_permissions(
    _path: PathBuf,
    _permissions: FilePropertiesPermissions,
) -> Result<FilePropertiesPermissions, String> {
    Err("permission editing is only available on Unix filesystems".to_owned())
}

fn read_directory_properties_contents(
    path: &Path,
) -> Result<FilePropertiesDirectoryContents, String> {
    let mut contents = FilePropertiesDirectoryContents {
        file_count: 0,
        directory_count: 0,
        total_size_bytes: 0,
        total_disk_size_bytes: 0,
    };

    for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            contents.directory_count += 1;
        } else {
            contents.file_count += 1;
        }
        contents.total_size_bytes = contents.total_size_bytes.saturating_add(metadata.len());
        contents.total_disk_size_bytes = contents
            .total_disk_size_bytes
            .saturating_add(metadata_disk_size(&metadata));
    }

    Ok(contents)
}

#[cfg(unix)]
fn metadata_disk_size(metadata: &std::fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn metadata_disk_size(metadata: &std::fs::Metadata) -> u64 {
    metadata.len()
}

async fn load_startup_environment() -> StartupEnvironment {
    startup_trace::mark_once("startup_environment_started");
    tokio::task::spawn_blocking(|| StartupEnvironment {
        home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        user_config: config::load_user_config(),
        state_database_path: config::default_state_database_path(),
    })
    .await
    .unwrap_or_else(|_| StartupEnvironment {
        home: PathBuf::from("/"),
        user_config: config::ui_thread_startup_config(),
        state_database_path: PathBuf::new(),
    })
}

async fn delayed_thumbnail_refresh(directory: PathBuf) -> PathBuf {
    tokio::time::sleep(THUMBNAIL_REFRESH_DELAY).await;
    directory
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

async fn load_operation_store(path: PathBuf) -> Result<LoadedOperationStore, String> {
    if path.as_os_str().is_empty() {
        return Err("state database path is unavailable".to_owned());
    }

    let store_outcome = tokio::task::spawn_blocking(move || {
        let store = TaskQueueStore::new(path)?;
        let restored_tasks = store.read_tasks()?;
        let column_width_overrides = store
            .read_column_widths()?
            .into_iter()
            .map(|(column_index, width)| {
                (column_index, config::normalize_column_width(width as f32))
            })
            .collect();
        Ok::<LoadedOperationStore, file_operation_store::StoreError>(LoadedOperationStore {
            task_queue_store: store,
            column_width_overrides,
            restored_tasks,
        })
    })
    .await
    .map_err(|error| error.to_string())?;
    store_outcome.map_err(|error| error.to_string())
}

async fn persist_column_width_overrides(
    task_queue_store: TaskQueueStore,
    column_width_overrides: HashMap<usize, f32>,
) -> Result<(), String> {
    let stored_widths = column_width_overrides
        .into_iter()
        .map(|(column_index, width)| {
            (
                column_index,
                f64::from(config::normalize_column_width(width)),
            )
        })
        .collect();
    tokio::task::spawn_blocking(move || task_queue_store.replace_column_widths(stored_widths))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
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

async fn load_sidebar_locations(
    home: PathBuf,
    configured_favorites: Option<Vec<config::SidebarFavoriteConfig>>,
) -> Vec<SidebarLocation> {
    let fallback_home = home.clone();
    let locations = tokio::task::spawn_blocking(move || {
        sidebar_locations(&home, configured_favorites.as_deref())
    })
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
