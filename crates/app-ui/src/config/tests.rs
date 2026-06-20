use std::fs;
use std::path::PathBuf;

use desktop_linux::{DisplayRendererGpu, DisplayRendererGpuClass, TerminalEmulator};
use file_core::FileOperationVerification;
use file_index::DirectoryErrorPolicy;

use super::*;

#[test]
fn parses_toml_user_config() {
    let parsed = parse_toml_user_config(
        r#"
search_index_dir = "/tmp/search-index"
search_index_directory_error_policy = "abort"
search_mode = "indexed"
search_mode_prompt = "completed"
thumbnail_cache_dir = "/tmp/thumbnails"
show_hidden_files = true
sidebar_width = 260.5
terminal_emulator = "ghostty"
rendering_backend = "gpu"
file_operation_verification = "strong"
browser_view_mode = "list"
"#,
        default_user_config(),
    );

    assert_eq!(parsed.search_index_dir, PathBuf::from("/tmp/search-index"));
    assert_eq!(
        parsed.search_index_directory_error_policy,
        DirectoryErrorPolicy::Abort
    );
    assert_eq!(parsed.search_mode, SearchBackendMode::Indexed);
    assert_eq!(parsed.search_mode_prompt, SearchModePromptStatus::Completed);
    assert_eq!(parsed.thumbnail_cache_dir, PathBuf::from("/tmp/thumbnails"));
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
}

#[test]
fn removed_off_verification_value_falls_back_to_default() {
    let parsed_toml = parse_toml_user_config(
        "file_operation_verification = \"off\"\n",
        default_user_config(),
    );

    assert_eq!(
        parsed_toml.file_operation_verification,
        default_user_config().file_operation_verification
    );
}

#[test]
fn serializes_basic_metadata_verification() {
    let mut config = default_user_config();
    config.file_operation_verification = FileOperationVerification::BasicMetadata;

    let content = toml_user_config_content(&config).unwrap();

    assert!(content.contains("file_operation_verification = \"basic_metadata\""));
    assert!(!content.contains("file_operation_verification = \"off\""));
}

#[test]
fn parses_display_gpu_preference() {
    let parsed = parse_toml_user_config("rendering_backend = \"display\"\n", default_user_config());

    assert_eq!(
        parsed.rendering_gpu_preference,
        RenderingGpuPreference::DisplayGpu
    );
}

#[test]
fn invalid_values_fall_back_to_defaults() {
    let default = default_user_config();
    let parsed = parse_toml_user_config(
        r#"
show_hidden_files = "maybe"
search_mode = "catalog"
search_mode_prompt = "later"
sidebar_width = "wide"
terminal_emulator = "missing"
rendering_backend = "metal"
file_operation_verification = "maybe"
browser_view_mode = "cover-flow"
"#,
        default.clone(),
    );

    assert_eq!(parsed.show_hidden_files, default.show_hidden_files);
    assert_eq!(parsed.search_mode, default.search_mode);
    assert_eq!(parsed.search_mode_prompt, default.search_mode_prompt);
    assert_eq!(parsed.sidebar_width, default.sidebar_width);
    assert_eq!(parsed.terminal_emulator, DEFAULT_TERMINAL_EMULATOR);
    assert_eq!(
        parsed.rendering_gpu_preference,
        DEFAULT_RENDERING_GPU_PREFERENCE
    );
    assert_eq!(
        parsed.file_operation_verification,
        DEFAULT_FILE_OPERATION_VERIFICATION
    );
    assert_eq!(parsed.browser_view_mode, BrowserViewMode::Columns);
}

#[test]
fn writes_toml_user_config_without_column_width_overrides() {
    let temp_dir = tempfile::tempdir().expect("create temp config dir");
    let path = temp_dir.path().join("config.toml");
    let mut config = default_user_config();
    config.rendering_gpu_preference = RenderingGpuPreference::HighPerformanceGpu;

    write_user_config(&path, &config).expect("write user config");

    let content = fs::read_to_string(path).expect("read user config");
    assert!(content.starts_with("# File Manager user configuration\n"));
    assert!(content.contains("sidebar_width = 180.0\n"));
    assert!(content.contains("search_mode = \"simple\"\n"));
    assert!(content.contains("search_mode_prompt = \"pending\"\n"));
    assert!(content.contains("search_index_directory_error_policy = \"skip_unreadable\"\n"));
    assert!(content.contains("rendering_backend = \"gpu\"\n"));
    assert!(content.contains("file_operation_verification = \"basic_metadata\"\n"));
    assert!(content.contains("browser_view_mode = \"columns\"\n"));
    assert!(!content.contains("[column_width_overrides]"));

    let parsed = parse_toml_user_config(&content, default_user_config());
    assert_eq!(
        parsed.rendering_gpu_preference,
        RenderingGpuPreference::HighPerformanceGpu
    );
    assert_eq!(
        parsed.file_operation_verification,
        DEFAULT_FILE_OPERATION_VERIFICATION
    );
    assert_eq!(parsed.browser_view_mode, BrowserViewMode::Columns);
}

#[test]
fn default_search_mode_is_simple_with_pending_prompt() {
    let config = default_user_config();

    assert_eq!(config.search_mode, SearchBackendMode::Simple);
    assert_eq!(config.search_mode_prompt, SearchModePromptStatus::Pending);
}

#[test]
fn search_mode_round_trips_through_toml() {
    let mut config = default_user_config();
    config.search_mode = SearchBackendMode::Indexed;
    config.search_mode_prompt = SearchModePromptStatus::Completed;

    let content = toml_user_config_content(&config).unwrap();
    let parsed = parse_toml_user_config(&content, default_user_config());

    assert_eq!(parsed.search_mode, SearchBackendMode::Indexed);
    assert_eq!(parsed.search_mode_prompt, SearchModePromptStatus::Completed);
}

#[test]
fn default_user_config_stores_default_search_index_excludes_as_editable_config() {
    assert_eq!(
        default_user_config().search_index_exclude_patterns,
        file_index::default_search_index_exclude_patterns()
            .iter()
            .map(|pattern| (*pattern).to_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn parses_empty_search_index_excludes_as_empty_config() {
    let parsed = parse_toml_user_config(
        "search_index_exclude_patterns = []\n",
        default_user_config(),
    );

    assert!(parsed.search_index_exclude_patterns.is_empty());
}

#[test]
fn normalizes_sidebar_width_from_config() {
    let narrow = parse_toml_user_config("sidebar_width = 20\n", default_user_config());
    let wide = parse_toml_user_config("sidebar_width = 1200\n", default_user_config());

    assert_eq!(narrow.sidebar_width, MIN_SIDEBAR_WIDTH);
    assert_eq!(wide.sidebar_width, MAX_SIDEBAR_WIDTH);
}

#[test]
fn parses_sidebar_favorites_from_toml() {
    let parsed = parse_toml_user_config(
        r#"
sidebar_favorites = [
  { label = "Downloads", path = "/home/user/Downloads" },
  { path = "/srv/projects" },
]
"#,
        default_user_config(),
    );

    let favorites = parsed.sidebar_favorites.expect("sidebar favorites");
    assert_eq!(favorites.len(), 2);
    assert_eq!(favorites[0].label, "Downloads");
    assert_eq!(favorites[0].path, PathBuf::from("/home/user/Downloads"));
    assert_eq!(favorites[1].label, "projects");
    assert_eq!(favorites[1].path, PathBuf::from("/srv/projects"));
}

#[test]
fn writes_sidebar_favorites_to_toml() {
    let temp_dir = tempfile::tempdir().expect("create temp config dir");
    let path = temp_dir.path().join("config.toml");
    let mut config = default_user_config();
    config.sidebar_favorites = Some(vec![SidebarFavoriteConfig {
        label: "Projects".to_owned(),
        path: PathBuf::from("/srv/projects"),
    }]);

    write_user_config(&path, &config).expect("write user config");

    let content = fs::read_to_string(path).expect("read user config");
    assert!(content.contains("sidebar_favorites"));
    let parsed = parse_toml_user_config(&content, default_user_config());
    assert_eq!(parsed.sidebar_favorites, config.sidebar_favorites);
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
