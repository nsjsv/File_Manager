use std::fs;
use std::path::PathBuf;

use desktop_linux::{
    DisplayRendererGpu, DisplayRendererGpuClass, NetworkConnection, NetworkConnectionId,
    NetworkProtocol, TerminalEmulator,
};
use file_core::{FileOperationVerification, SortDirection, SortField};
use file_index::{DirectoryErrorPolicy, MediaMetadataScope};
use file_operation_store::{StoredNetworkConnection, TaskQueueStore};

use crate::model::{BrowserViewMode, ListColumnKind, ListDirectorySizeDisplayMode};
use crate::network_connections::SavedNetworkConnection;
use crate::shortcuts::ShortcutBindingId;

use super::*;

#[test]
fn parses_legacy_toml_user_config_for_migration() {
    let parsed = legacy_toml::parse_toml_user_config(
        r#"
search_index_dir = "/tmp/search-index"
search_index_directory_error_policy = "abort"
search_index_media_scope = "images"
search_mode = "indexed"
search_mode_prompt = "completed"
thumbnail_cache_dir = "/tmp/thumbnails"
network_list_thumbnail_downloads_enabled = true
max_preview_file_bytes = 4194304
show_hidden_files = true
sidebar_width = 260.5
terminal_emulator = "ghostty"
rendering_backend = "gpu"
file_operation_verification = "strong"
browser_view_mode = "list"
startup_location = "custom"
startup_custom_directory = "/workspace"
save_view_state = true
shortcuts = { focus_path_input = "Ctrl+Alt+L" }
sidebar_favorites = [
  { label = "Downloads", path = "/home/user/Downloads" },
  { path = "/srv/projects" },
]
network_connections = [
  { id = "nas", label = "NAS", protocol = "smb", uri = "smb://server/share" },
  { id = "docs", label = "", protocol = "webdav", uri = "https://user@example.test/docs", auto_connect = true },
  { id = "sftp", label = "SFTP", protocol = "sftp", uri = "sftp://user@sftp.example.test/srv/share" },
]
"#,
        default_user_config(),
    );

    assert_eq!(parsed.search_index_dir, PathBuf::from("/tmp/search-index"));
    assert_eq!(
        parsed.search_index_directory_error_policy,
        DirectoryErrorPolicy::Abort
    );
    assert_eq!(parsed.search_index_media_scope, MediaMetadataScope::Images);
    assert_eq!(parsed.search_mode, SearchBackendMode::Indexed);
    assert_eq!(parsed.search_mode_prompt, SearchModePromptStatus::Completed);
    assert_eq!(parsed.thumbnail_cache_dir, PathBuf::from("/tmp/thumbnails"));
    assert!(parsed.network_list_thumbnail_downloads_enabled);
    assert_eq!(parsed.max_preview_file_bytes, 4 * 1024 * 1024);
    assert!(parsed.show_hidden_files);
    assert_eq!(parsed.sidebar_width, 260.5);
    assert_eq!(parsed.terminal_emulator, TerminalEmulator::Ghostty);
    assert_eq!(
        parsed.rendering_gpu_preference,
        RenderingGpuPreference::HighPerformanceGpu
    );
    assert_eq!(
        parsed.file_operation_verification,
        FileOperationVerification::Strong
    );
    assert_eq!(parsed.browser_view_mode, BrowserViewMode::List);
    assert_eq!(
        parsed.startup_location_policy,
        StartupLocationPolicy::CustomDirectory
    );
    assert_eq!(parsed.startup_custom_directory, PathBuf::from("/workspace"));
    assert!(!parsed.save_view_state);
    assert_eq!(
        parsed
            .shortcuts
            .binding(ShortcutBindingId::FocusPathInput)
            .config_value(),
        "Ctrl+Alt+L"
    );
    assert_eq!(parsed.sidebar_favorites.as_ref().map(Vec::len), Some(2));
    assert_eq!(parsed.network_connections.len(), 3);
    assert_eq!(
        parsed.network_connections[1].connection.uri,
        "davs://user@example.test/docs"
    );
    assert!(parsed.network_connections[1].auto_connect);
    assert_eq!(
        parsed.list_directory_size_display_mode,
        ListDirectorySizeDisplayMode::ItemCount
    );
}

#[test]
fn app_config_toml_contains_only_startup_level_keys() {
    let app_config = AppConfig {
        search_index_dir: PathBuf::from("/tmp/search-index"),
        thumbnail_cache_dir: PathBuf::from("/tmp/thumbnails"),
        rendering_gpu_preference: RenderingGpuPreference::HighPerformanceGpu,
    };

    let content = app_config::toml_app_config_content(&app_config).unwrap();
    let document = content.parse::<toml::Table>().unwrap();

    assert_eq!(document.len(), 3);
    assert_eq!(
        document
            .get("search_index_dir")
            .and_then(toml::Value::as_str),
        Some("/tmp/search-index")
    );
    assert_eq!(
        document
            .get("thumbnail_cache_dir")
            .and_then(toml::Value::as_str),
        Some("/tmp/thumbnails")
    );
    assert_eq!(
        document
            .get("rendering_backend")
            .and_then(toml::Value::as_str),
        Some("gpu")
    );
    for key in [
        "show_hidden_files",
        "sidebar_width",
        "sidebar_favorites",
        "network_connections",
        "shortcuts",
        "startup_location",
        "search_mode",
        "max_preview_file_bytes",
        "list_directory_size_display_mode",
    ] {
        assert!(
            !document.contains_key(key),
            "app config must not contain {key}"
        );
    }

    let parsed = app_config::parse_toml_app_config(&content, app_config::default_app_config());
    assert_eq!(parsed, app_config);
}

#[test]
fn writes_app_config_without_user_preferences() {
    let temp_dir = tempfile::tempdir().expect("create temp config dir");
    let path = temp_dir.path().join("config.toml");
    let app_config = AppConfig {
        search_index_dir: temp_dir.path().join("search-index"),
        thumbnail_cache_dir: temp_dir.path().join("thumbnails"),
        rendering_gpu_preference: RenderingGpuPreference::DisplayGpu,
    };

    app_config::write_app_config(&path, &app_config).expect("write app config");

    let content = fs::read_to_string(path).expect("read app config");
    assert!(content.starts_with("# File Manager application configuration\n"));
    assert!(content.contains("rendering_backend = \"display\""));
    assert!(!content.contains("show_hidden_files"));
    assert!(!content.contains("shortcuts"));
}

#[test]
fn user_preferences_round_trip_through_sqlite() {
    let temp_dir = tempfile::tempdir().expect("create temp state dir");
    let state_database_path = temp_dir.path().join("state.sqlite");
    let store = TaskQueueStore::new(&state_database_path).expect("create state store");
    let app_config = AppConfig {
        search_index_dir: PathBuf::from("/var/lib/file-manager/search"),
        thumbnail_cache_dir: PathBuf::from("/var/cache/file-manager/thumbs"),
        rendering_gpu_preference: RenderingGpuPreference::HighPerformanceGpu,
    };
    let mut config = default_user_config();
    config.show_hidden_files = true;
    config.sidebar_width = 245.0;
    config.sidebar_favorites = Some(vec![SidebarFavoriteConfig {
        label: "Projects".to_owned(),
        path: PathBuf::from("/srv/projects"),
    }]);
    config.network_connections = vec![SavedNetworkConnection::new(
        NetworkConnection::new(
            NetworkConnectionId::new("nas"),
            "NAS",
            NetworkProtocol::Smb,
            "smb://server/share",
        )
        .unwrap(),
        true,
    )];
    config.startup_location_policy = StartupLocationPolicy::PreviousSession;
    config.startup_custom_directory = PathBuf::from("/workspace");
    config.save_view_state = config.startup_location_policy.saves_view_state();
    config.browser_view_mode = BrowserViewMode::List;
    config
        .list_view_preferences
        .set_column_visible(ListColumnKind::Kind, false);
    config
        .list_view_preferences
        .set_column_visible(ListColumnKind::Extension, true);
    config
        .list_view_preferences
        .set_column_width(ListColumnKind::Name, 280.0);
    config
        .list_view_preferences
        .set_column_width(ListColumnKind::Extension, 140.0);
    config
        .list_view_preferences
        .move_column_right(ListColumnKind::Name);
    config
        .list_view_preferences
        .move_column_to(ListColumnKind::Extension, ListColumnKind::Name);
    config
        .list_view_preferences
        .select_sort_column(ListColumnKind::Size);
    config.list_directory_size_display_mode = ListDirectorySizeDisplayMode::RecursiveTotalSize;
    config.terminal_emulator = TerminalEmulator::Ghostty;
    config.file_operation_verification = FileOperationVerification::Strong;
    config.search_mode = SearchBackendMode::Indexed;
    config.search_mode_prompt = SearchModePromptStatus::Completed;
    config.search_index_content_enabled = true;
    config.search_index_media_scope = MediaMetadataScope::Images;
    config.search_index_directory_error_policy = DirectoryErrorPolicy::Abort;
    config.network_list_thumbnail_downloads_enabled = true;
    config.max_preview_file_bytes = 8 * 1024 * 1024;
    let mut shortcut_table = toml::Table::new();
    shortcut_table.insert(
        "focus_path_input".to_owned(),
        toml::Value::String("Ctrl+Alt+L".to_owned()),
    );
    config.shortcuts.apply_toml_table(&shortcut_table);

    save_user_preferences(&store, &config.user_preferences()).expect("save preferences");

    let loaded = user_preferences::load_user_config_from_sources(
        app_config.clone(),
        &state_database_path,
        None,
    );

    assert_eq!(loaded.search_index_dir, app_config.search_index_dir);
    assert_eq!(loaded.thumbnail_cache_dir, app_config.thumbnail_cache_dir);
    assert_eq!(
        loaded.rendering_gpu_preference,
        app_config.rendering_gpu_preference
    );
    assert!(loaded.show_hidden_files);
    assert_eq!(loaded.sidebar_width, 245.0);
    assert_eq!(loaded.sidebar_favorites, config.sidebar_favorites);
    assert_eq!(loaded.network_connections, config.network_connections);
    assert_eq!(
        loaded.startup_location_policy,
        StartupLocationPolicy::PreviousSession
    );
    assert_eq!(loaded.startup_custom_directory, PathBuf::from("/workspace"));
    assert!(loaded.save_view_state);
    assert_eq!(loaded.browser_view_mode, BrowserViewMode::List);
    let loaded_columns = loaded
        .list_view_preferences
        .columns()
        .iter()
        .map(|column| (column.kind, column.visible, column.width))
        .collect::<Vec<_>>();
    assert_eq!(loaded_columns[0].0, ListColumnKind::Modified);
    assert_eq!(loaded_columns[1].0, ListColumnKind::Extension);
    assert!(loaded_columns[1].1);
    assert_eq!(loaded_columns[1].2, 140.0);
    assert_eq!(loaded_columns[2].0, ListColumnKind::Name);
    assert_eq!(loaded_columns[2].2, 280.0);
    assert!(loaded
        .list_view_preferences
        .columns()
        .iter()
        .find(|column| column.kind == ListColumnKind::Kind)
        .is_some_and(|column| !column.visible));
    assert_eq!(loaded.list_view_preferences.sort().field, SortField::Size);
    assert_eq!(
        loaded.list_view_preferences.sort().direction,
        SortDirection::Ascending
    );
    assert_eq!(
        loaded.list_directory_size_display_mode,
        ListDirectorySizeDisplayMode::RecursiveTotalSize
    );
    assert_eq!(loaded.terminal_emulator, TerminalEmulator::Ghostty);
    assert_eq!(
        loaded.file_operation_verification,
        FileOperationVerification::Strong
    );
    assert_eq!(loaded.search_mode, SearchBackendMode::Indexed);
    assert_eq!(loaded.search_mode_prompt, SearchModePromptStatus::Completed);
    assert!(loaded.search_index_content_enabled);
    assert_eq!(loaded.search_index_media_scope, MediaMetadataScope::Images);
    assert_eq!(
        loaded.search_index_directory_error_policy,
        DirectoryErrorPolicy::Abort
    );
    assert!(loaded.network_list_thumbnail_downloads_enabled);
    assert_eq!(loaded.max_preview_file_bytes, 8 * 1024 * 1024);
    assert_eq!(
        loaded
            .shortcuts
            .binding(ShortcutBindingId::FocusPathInput)
            .config_value(),
        "Ctrl+Alt+L"
    );
}

#[test]
fn stored_preferences_skip_password_bearing_network_connections() {
    let default = default_user_config();
    let mut stored = default.user_preferences().to_stored();
    stored.network_connections = vec![
        StoredNetworkConnection {
            id: "bad".to_owned(),
            label: "Bad".to_owned(),
            protocol: "webdav".to_owned(),
            uri: "davs://user:secret@example.test/docs".to_owned(),
            auto_connect: true,
        },
        StoredNetworkConnection {
            id: "good".to_owned(),
            label: "Good".to_owned(),
            protocol: "smb".to_owned(),
            uri: "smb://server/share".to_owned(),
            auto_connect: false,
        },
    ];

    let preferences = UserPreferences::from_stored(stored, &default);

    assert_eq!(preferences.network_connections.len(), 1);
    assert_eq!(
        preferences.network_connections[0].connection.id.as_str(),
        "good"
    );
}

#[test]
fn stored_preferences_derive_view_state_saving_from_startup_location() {
    let default = default_user_config();
    let mut stored = default.user_preferences().to_stored();
    stored.startup_location = "custom".to_owned();
    stored.save_view_state = true;

    let preferences = UserPreferences::from_stored(stored, &default);

    assert_eq!(
        preferences.startup_location_policy,
        StartupLocationPolicy::CustomDirectory
    );
    assert!(!preferences.save_view_state);
}

#[test]
fn stored_preferences_default_list_directory_size_mode_is_item_count() {
    let default = default_user_config();
    let mut stored = default.user_preferences().to_stored();
    stored.list_directory_size_display_mode = "unknown".to_owned();

    let preferences = UserPreferences::from_stored(stored, &default);

    assert_eq!(
        preferences.list_directory_size_display_mode,
        ListDirectorySizeDisplayMode::ItemCount
    );
}

#[test]
fn migrates_legacy_toml_preferences_to_sqlite_once() {
    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let config_dir = temp_dir.path().join("config");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join(CONFIG_FILE_NAME),
        r#"
search_index_dir = "/tmp/search-index"
thumbnail_cache_dir = "/tmp/thumbnails"
rendering_backend = "gpu"
show_hidden_files = true
sidebar_width = 250.0
startup_location = "previous_session"
startup_custom_directory = "/workspace"
save_view_state = true
shortcuts = { focus_path_input = "Ctrl+Alt+L" }
sidebar_favorites = [{ label = "Projects", path = "/srv/projects" }]
network_connections = [
  { id = "bad", label = "Bad", protocol = "webdav", uri = "davs://user:secret@example.test/docs", auto_connect = true },
  { id = "good", label = "Good", protocol = "smb", uri = "smb://server/share" },
]
"#,
    )
    .expect("write legacy config");
    let app_config =
        app_config::load_app_config_from_dir(&config_dir, app_config::default_app_config());
    let state_database_path = temp_dir.path().join("state.sqlite");

    let loaded = user_preferences::load_user_config_from_sources(
        app_config,
        &state_database_path,
        Some(&config_dir),
    );

    assert!(loaded.show_hidden_files);
    assert_eq!(loaded.sidebar_width, 250.0);
    assert_eq!(
        loaded.startup_location_policy,
        StartupLocationPolicy::PreviousSession
    );
    assert_eq!(loaded.startup_custom_directory, PathBuf::from("/workspace"));
    assert!(loaded.save_view_state);
    assert_eq!(loaded.sidebar_favorites.as_ref().map(Vec::len), Some(1));
    assert_eq!(loaded.network_connections.len(), 1);
    assert_eq!(loaded.network_connections[0].connection.id.as_str(), "good");
    assert_eq!(
        loaded
            .shortcuts
            .binding(ShortcutBindingId::FocusPathInput)
            .config_value(),
        "Ctrl+Alt+L"
    );

    let store = TaskQueueStore::new(&state_database_path).expect("open migrated store");
    let stored = store
        .read_user_preferences()
        .expect("read migrated preferences")
        .expect("migrated preferences");
    assert_eq!(stored.network_connections.len(), 1);
    assert_eq!(stored.network_connections[0].id, "good");
    assert!(!stored.network_connections[0].uri.contains("secret"));
}

#[test]
fn invalid_legacy_values_fall_back_to_defaults() {
    let default = default_user_config();
    let parsed = legacy_toml::parse_toml_user_config(
        r#"
show_hidden_files = "maybe"
network_list_thumbnail_downloads_enabled = "maybe"
max_preview_file_bytes = 0
search_mode = "catalog"
search_mode_prompt = "later"
sidebar_width = "wide"
terminal_emulator = "missing"
rendering_backend = "metal"
file_operation_verification = "maybe"
browser_view_mode = "cover-flow"
startup_location = "moon"
startup_custom_directory = ""
save_view_state = "maybe"
"#,
        default.clone(),
    );

    assert_eq!(parsed.show_hidden_files, default.show_hidden_files);
    assert_eq!(
        parsed.network_list_thumbnail_downloads_enabled,
        default.network_list_thumbnail_downloads_enabled
    );
    assert_eq!(
        parsed.max_preview_file_bytes,
        default.max_preview_file_bytes
    );
    assert_eq!(parsed.search_mode, default.search_mode);
    assert_eq!(parsed.search_mode_prompt, default.search_mode_prompt);
    assert_eq!(parsed.sidebar_width, default.sidebar_width);
    assert_eq!(parsed.terminal_emulator, default.terminal_emulator);
    assert_eq!(
        parsed.rendering_gpu_preference,
        default.rendering_gpu_preference
    );
    assert_eq!(
        parsed.file_operation_verification,
        default.file_operation_verification
    );
    assert_eq!(parsed.browser_view_mode, default.browser_view_mode);
    assert_eq!(
        parsed.startup_location_policy,
        default.startup_location_policy
    );
    assert_eq!(
        parsed.startup_custom_directory,
        default.startup_custom_directory
    );
    assert_eq!(parsed.save_view_state, default.save_view_state);
    assert_eq!(
        parsed.list_directory_size_display_mode,
        default.list_directory_size_display_mode
    );
}

#[test]
fn default_user_config_keeps_expected_search_and_preview_defaults() {
    let config = default_user_config();

    assert_eq!(config.search_mode, SearchBackendMode::Simple);
    assert_eq!(config.search_mode_prompt, SearchModePromptStatus::Pending);
    assert!(!config.network_list_thumbnail_downloads_enabled);
    assert_eq!(
        config.max_preview_file_bytes,
        DEFAULT_MAX_PREVIEW_FILE_BYTES
    );
    assert_eq!(config.startup_location_policy, StartupLocationPolicy::Home);
    assert!(!config.save_view_state);
    assert_eq!(
        config.list_directory_size_display_mode,
        ListDirectorySizeDisplayMode::ItemCount
    );
    assert_eq!(
        config.search_index_exclude_patterns,
        file_index::default_search_index_exclude_patterns()
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn normalizes_legacy_sidebar_width_from_config() {
    let narrow = legacy_toml::parse_toml_user_config("sidebar_width = 20\n", default_user_config());
    let wide = legacy_toml::parse_toml_user_config("sidebar_width = 1200\n", default_user_config());

    assert_eq!(narrow.sidebar_width, MIN_SIDEBAR_WIDTH);
    assert_eq!(wide.sidebar_width, MAX_SIDEBAR_WIDTH);
}

#[test]
fn maps_rendering_gpu_preferences_to_iced_backend() {
    assert_eq!(
        RenderingGpuPreference::DisplayGpu.iced_backend_candidates(),
        "wgpu"
    );
    assert_eq!(
        RenderingGpuPreference::HighPerformanceGpu.iced_backend_candidates(),
        "wgpu"
    );
}

#[test]
fn maps_display_gpu_preference_to_display_matched_wgpu_power() {
    let integrated_gpu =
        DisplayRendererGpu::from_drm_ids(DisplayRendererGpuClass::Integrated, "0x1002", "0x15bf");
    let discrete_gpu =
        DisplayRendererGpu::from_drm_ids(DisplayRendererGpuClass::Discrete, "0x10de", "0x28e0");

    assert_eq!(
        RenderingGpuPreference::DisplayGpu.wgpu_power_preference(Some(&integrated_gpu)),
        Some("low")
    );
    assert_eq!(
        RenderingGpuPreference::DisplayGpu.wgpu_power_preference(Some(&discrete_gpu)),
        Some("high")
    );
    assert_eq!(
        RenderingGpuPreference::DisplayGpu.wgpu_power_preference(None),
        Some("none")
    );
    assert_eq!(
        RenderingGpuPreference::HighPerformanceGpu.wgpu_power_preference(Some(&integrated_gpu)),
        Some("high")
    );
}

#[test]
fn maps_display_gpu_preference_to_forced_vulkan_device_selection() {
    let integrated_gpu =
        DisplayRendererGpu::from_drm_ids(DisplayRendererGpuClass::Integrated, "0x1002", "0x15bf");

    assert_eq!(
        RenderingGpuPreference::DisplayGpu.mesa_vulkan_device_select(Some(&integrated_gpu)),
        Some(String::from("1002:15bf!"))
    );
    assert_eq!(
        RenderingGpuPreference::DisplayGpu.mesa_vulkan_device_select(None),
        None
    );
    assert_eq!(
        RenderingGpuPreference::HighPerformanceGpu.mesa_vulkan_device_select(Some(&integrated_gpu)),
        None
    );
}

#[test]
fn defaults_to_display_gpu_preference() {
    assert_eq!(
        default_user_config().rendering_gpu_preference,
        RenderingGpuPreference::DisplayGpu
    );
}
