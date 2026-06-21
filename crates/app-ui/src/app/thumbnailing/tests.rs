use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use desktop_linux::{NetworkConnection, NetworkConnectionId, NetworkMountState, NetworkProtocol};
use file_core::{DirectoryEntry, EntryMetadata, FileKind};

use super::*;
use crate::animated_image_preview::{
    AnimatedImageFrame, AnimatedImagePlayback, AnimatedImagePreview,
};
use crate::config::ui_thread_startup_config;
use crate::model::{BrowserPaneLayout, BrowserTab, SplitAxis};
use crate::network_connections::NetworkConnectionState;

#[test]
fn missing_viewport_schedules_initial_thumbnail_rows() {
    let range = thumbnail_range_for_row_height(None, 100, crate::list_view::LIST_ROW_HEIGHT);

    assert_eq!(range, (0, INITIAL_THUMBNAIL_ROWS));
}

#[test]
fn measured_viewport_schedules_visible_rows_with_overscan() {
    let viewport = ColumnViewport {
        offset_y: crate::list_view::LIST_ROW_HEIGHT * 40.0,
        height: crate::list_view::LIST_ROW_HEIGHT * 3.0,
    };

    let range =
        thumbnail_range_for_row_height(Some(viewport), 120, crate::list_view::LIST_ROW_HEIGHT);

    assert_eq!(range, (12, 71));
}

#[test]
fn column_thumbnail_range_uses_column_row_height() {
    let viewport = ColumnViewport {
        offset_y: crate::three_column_view::COLUMN_ENTRY_HEIGHT * 40.0,
        height: crate::three_column_view::COLUMN_ENTRY_HEIGHT * 3.0,
    };

    let range = thumbnail_range_for_row_height(
        Some(viewport),
        120,
        crate::three_column_view::COLUMN_ENTRY_HEIGHT,
    );

    assert_eq!(range, (12, 71));
}

#[test]
fn inactive_pane_thumbnail_request_matches_current_entry() {
    let (browser, _, _, image_entry) = browser_with_inactive_image_pane();
    let request = request_for_entry(&image_entry, LIST_THUMBNAIL_EDGE).expect("image request");

    assert!(browser.is_current_thumbnail_request(&request));
}

#[test]
fn inactive_pane_thumbnail_range_uses_its_own_viewport() {
    let (browser, inactive_id, inactive_dir, image_entry) = browser_with_inactive_image_pane();

    let requests =
        browser.thumbnail_requests_for_pane_directory_range(inactive_id, inactive_dir.as_path());

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].source, image_entry.path);
}

#[test]
fn inactive_pane_thumbnail_range_schedules_svg_request() {
    let (browser, inactive_id, inactive_dir, image_entry) =
        browser_with_inactive_pane_image("/inactive/vector.svg");

    let requests =
        browser.thumbnail_requests_for_pane_directory_range(inactive_id, inactive_dir.as_path());

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].source, image_entry.path);
}

#[test]
fn list_scrolled_schedules_visible_list_thumbnail_requests() {
    let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
    browser.view_mode = BrowserViewMode::List;
    browser.entries = vec![
        image_entry("/workspace/photo-0.png"),
        image_entry("/workspace/vector.svg"),
        image_entry("/workspace/photo-2.png"),
    ];

    browser.schedule_visible_list_thumbnail_range_for_pane(
        BrowserPaneId::PRIMARY,
        Some(ColumnViewport {
            offset_y: 0.0,
            height: crate::list_view::LIST_ROW_HEIGHT * 3.0,
        }),
    );

    let batch = browser
        .thumbnail_cache
        .take_next_batch()
        .into_iter()
        .collect::<Vec<_>>();
    let queued_sources = batch
        .iter()
        .map(|work| work.request.source.clone())
        .collect::<HashSet<_>>();

    assert!(queued_sources.contains(&PathBuf::from("/workspace/photo-0.png")));
    assert!(queued_sources.contains(&PathBuf::from("/workspace/vector.svg")));
    assert!(batch
        .iter()
        .all(|work| work.load_policy == ThumbnailLoadPolicy::LoadOrGenerate));
}

#[test]
fn network_list_thumbnails_default_to_cache_only_policy() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    browser.view_mode = BrowserViewMode::List;
    browser.entries = vec![image_entry("/mnt/nas/photo.png")];
    mount_network_root(&mut browser, "/mnt/nas");

    browser.schedule_visible_list_thumbnail_range_for_pane(
        BrowserPaneId::PRIMARY,
        Some(ColumnViewport {
            offset_y: 0.0,
            height: crate::list_view::LIST_ROW_HEIGHT,
        }),
    );

    let batch = browser.thumbnail_cache.take_next_batch();

    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].load_policy, ThumbnailLoadPolicy::CacheOnly);
}

#[test]
fn enabled_network_list_thumbnails_use_generate_policy() {
    let mut config = crate::config::default_user_config();
    config.network_list_thumbnail_downloads_enabled = true;
    let (mut browser, _) = FileBrowser::new(config);
    browser.view_mode = BrowserViewMode::List;
    browser.entries = vec![image_entry("/mnt/nas/photo.png")];
    mount_network_root(&mut browser, "/mnt/nas");

    browser.schedule_visible_list_thumbnail_range_for_pane(
        BrowserPaneId::PRIMARY,
        Some(ColumnViewport {
            offset_y: 0.0,
            height: crate::list_view::LIST_ROW_HEIGHT,
        }),
    );

    let batch = browser.thumbnail_cache.take_next_batch();

    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].load_policy, ThumbnailLoadPolicy::LoadOrGenerate);
}

#[test]
fn cache_only_thumbnail_miss_does_not_block_later_generation() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    let image_entry = image_entry("/mnt/nas/photo.png");
    let request = request_for_entry(&image_entry, LIST_THUMBNAIL_EDGE).expect("request");
    browser.thumbnail_cache.enqueue_cached_request(
        request.clone(),
        ThumbnailPurpose::List,
        ThumbnailPriority::Visible,
    );
    let work = browser
        .thumbnail_cache
        .take_next_batch()
        .pop()
        .expect("cache-only work");

    drop(browser.accept_thumbnail_batch(vec![ThumbnailLoadOutcome {
        work,
        result: ThumbnailLoadResult::CacheMiss,
    }]));
    browser.thumbnail_cache.enqueue_request(
        request,
        ThumbnailPurpose::List,
        ThumbnailPriority::Visible,
    );

    assert_eq!(browser.thumbnail_cache.take_next_batch().len(), 1);
}

#[test]
fn preview_thumbnail_refresh_skips_same_edge_window_resize() {
    let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
    let image_entry = image_entry("/workspace/vector.svg");
    browser.entries = vec![image_entry.clone()];
    browser.preview_size = crate::model::PreviewSize {
        width: 640.0,
        height: 480.0,
    };
    browser.preview = Some(PreviewState::Ready(PreviewContent::Image {
        path: image_entry.path.clone(),
        handle: iced::widget::image::Handle::from_path("/tmp/vector-thumb.png"),
        width: 320,
        height: 240,
        max_edge: 640,
    }));

    let command = browser.refresh_preview_thumbnail_for_size();

    assert_eq!(command.units(), 0);
}

#[test]
fn preview_thumbnail_refresh_skips_animated_image_preview() {
    let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
    browser.preview_size = crate::model::PreviewSize {
        width: 1400.0,
        height: 1000.0,
    };
    let animated_path = PathBuf::from("/workspace/loop.gif");
    let first_frame = AnimatedImageFrame {
        path: animated_path.clone(),
        generation: 1,
        position: std::time::Duration::ZERO,
        handle: iced::widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 255]),
        width: 1,
        height: 1,
    };
    browser.preview = Some(PreviewState::Ready(PreviewContent::AnimatedImage(
        AnimatedImagePreview::new(
            animated_path,
            first_frame,
            1,
            Some(std::time::Duration::from_millis(40)),
            AnimatedImagePlayback::Animated,
        )
        .expect("animated image preview"),
    )));

    let command = browser.refresh_preview_thumbnail_for_size();

    assert_eq!(command.units(), 0);
}

fn browser_with_inactive_image_pane() -> (FileBrowser, BrowserPaneId, PathBuf, DirectoryEntry) {
    browser_with_inactive_pane_image("/inactive/photo.png")
}

fn browser_with_inactive_pane_image(
    image_path: &str,
) -> (FileBrowser, BrowserPaneId, PathBuf, DirectoryEntry) {
    let (mut browser, _) = FileBrowser::new(ui_thread_startup_config());
    let inactive_id = BrowserPaneId(1);
    let inactive_dir = PathBuf::from("/inactive");
    let image_entry = image_entry(image_path);
    let tab = BrowserTab::directory(1, inactive_dir.clone());

    browser.panes.push(BrowserPane {
        id: inactive_id,
        current_dir: inactive_dir.clone(),
        is_trash_view: false,
        entries: vec![image_entry.clone()],
        directory_loading_placeholder_entries: Vec::new(),
        trash_entries: Vec::new(),
        selected: None,
        selected_paths: HashSet::new(),
        selection_anchor: None,
        deepest_open_column_directory: None,
        expanded_directories: HashMap::new(),
        view_mode: crate::model::BrowserViewMode::Columns,
        column_viewports: HashMap::from([(
            inactive_dir.clone(),
            ColumnViewport {
                offset_y: 0.0,
                height: crate::list_view::LIST_ROW_HEIGHT,
            },
        )]),
        tabs: vec![tab.clone()],
        active_tab_id: tab.id,
        path_input: inactive_dir.to_string_lossy().into_owned(),
        path_suggestions: Vec::new(),
        path_suggestion_selection: None,
        path_suggestion_generation: 0,
        directory_load_generation: 0,
        directory_load_cancel: None,
        back_stack: Vec::new(),
        forward_stack: Vec::new(),
        is_loading: false,
    });
    browser.pane_layout = BrowserPaneLayout::Split {
        axis: SplitAxis::Horizontal,
        first: BrowserPaneId::PRIMARY,
        second: inactive_id,
        active: BrowserPaneId::PRIMARY,
    };

    (browser, inactive_id, inactive_dir, image_entry)
}

fn image_entry(path: &str) -> DirectoryEntry {
    DirectoryEntry::new(
        PathBuf::from(path),
        FileKind::File,
        EntryMetadata {
            len: 10,
            modified: None,
            readonly: false,
        },
        false,
        false,
        false,
    )
}

fn mount_network_root(browser: &mut FileBrowser, mount_path: &str) {
    let id = NetworkConnectionId::new("nas");
    let connection = NetworkConnection::new(
        id.clone(),
        "NAS",
        NetworkProtocol::Smb,
        "smb://server/share",
    )
    .expect("network connection");
    browser.network_connections = NetworkConnectionState::from_connections(vec![connection]);
    browser.network_connections.accept_loaded(vec![(
        id,
        NetworkMountState::Mounted(PathBuf::from(mount_path)),
    )]);
}
