use std::fs;
use std::path::PathBuf;

use desktop_linux::{DisplayRendererGpu, DisplayRendererGpuClass, TerminalEmulator};
use file_core::FileOperationVerification;

use super::*;

#[test]
fn parses_toml_user_config() {
    let parsed = parse_toml_user_config(
        r#"
search_index_dir = "/tmp/search-index"
thumbnail_cache_dir = "/tmp/thumbnails"
show_hidden_files = true
startup_index_prompt = "completed"
sidebar_width = 260.5
terminal_emulator = "ghostty"
rendering_backend = "gpu"
file_operation_verification = "strong"
browser_view_mode = "list"

[column_width_overrides]
0 = 240.5
2 = 360
"#,
        default_user_config(),
    );

    assert_eq!(parsed.search_index_dir, PathBuf::from("/tmp/search-index"));
    assert_eq!(parsed.thumbnail_cache_dir, PathBuf::from("/tmp/thumbnails"));
    assert!(parsed.show_hidden_files);
    assert_eq!(
        parsed.startup_index_prompt,
        StartupIndexPromptStatus::Completed
    );
    assert_eq!(parsed.sidebar_width, 260.5);
    assert_eq!(parsed.legacy_column_width_overrides.get(&0), Some(&240.5));
    assert_eq!(parsed.legacy_column_width_overrides.get(&2), Some(&360.0));
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
fn parses_legacy_user_config() {
    let parsed = parse_legacy_user_config(
        "show_hidden_files=true\nstartup_index_prompt=completed\nsidebar_width=260.5\ncolumn_width_overrides=0:240.5,2:360\nterminal_emulator=ghostty\nrendering_backend=gpu\nfile_operation_verification=strong\nbrowser_view_mode=list\n",
        default_user_config(),
    );

    assert!(parsed.show_hidden_files);
    assert_eq!(
        parsed.startup_index_prompt,
        StartupIndexPromptStatus::Completed
    );
    assert_eq!(parsed.sidebar_width, 260.5);
    assert_eq!(parsed.legacy_column_width_overrides.get(&0), Some(&240.5));
    assert_eq!(parsed.legacy_column_width_overrides.get(&2), Some(&360.0));
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
fn maps_removed_off_verification_to_basic_metadata() {
    let parsed_toml = parse_toml_user_config(
        "file_operation_verification = \"off\"\n",
        default_user_config(),
    );
    let parsed_legacy =
        parse_legacy_user_config("file_operation_verification=off\n", default_user_config());

    assert_eq!(
        parsed_toml.file_operation_verification,
        FileOperationVerification::BasicMetadata
    );
    assert_eq!(
        parsed_legacy.file_operation_verification,
        FileOperationVerification::BasicMetadata
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
fn parses_legacy_software_as_display_gpu_preference() {
    let parsed =
        parse_toml_user_config("rendering_backend = \"software\"\n", default_user_config());

    assert_eq!(
        parsed.rendering_gpu_preference,
        RenderingGpuPreference::DisplayGpu
    );
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
fn invalid_column_width_overrides_fall_back_to_empty() {
    let default = default_user_config();
    let parsed = parse_toml_user_config(
        r#"
show_hidden_files = "maybe"
startup_index_prompt = "later"
sidebar_width = "wide"
column_width_overrides = "bad"
terminal_emulator = "missing"
rendering_backend = "metal"
file_operation_verification = "maybe"
browser_view_mode = "cover-flow"
"#,
        default.clone(),
    );

    assert_eq!(parsed.show_hidden_files, default.show_hidden_files);
    assert_eq!(parsed.startup_index_prompt, default.startup_index_prompt);
    assert_eq!(parsed.sidebar_width, default.sidebar_width);
    assert!(parsed.legacy_column_width_overrides.is_empty());
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
    config.legacy_column_width_overrides.insert(0, 240.0);

    write_user_config(&path, &config).expect("write user config");

    let content = fs::read_to_string(path).expect("read user config");
    assert!(content.starts_with("# File Manager user configuration\n"));
    assert!(content.contains("sidebar_width = 180.0\n"));
    assert!(content.contains("startup_index_prompt = \"pending\"\n"));
    assert!(content.contains("rendering_backend = \"gpu\"\n"));
    assert!(content.contains("file_operation_verification = \"basic_metadata\"\n"));
    assert!(content.contains("browser_view_mode = \"columns\"\n"));
    assert!(!content.contains("[column_width_overrides]"));

    let parsed = parse_toml_user_config(&content, default_user_config());
    assert!(parsed.legacy_column_width_overrides.is_empty());
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
fn loads_legacy_config_when_toml_is_missing() {
    let temp_dir = tempfile::tempdir().expect("create temp config dir");
    fs::write(
        temp_dir.path().join("config.txt"),
        "show_hidden_files=true\nterminal_emulator=ghostty\n",
    )
    .expect("write legacy config");

    let parsed = load_user_config_from_dir(temp_dir.path(), default_user_config());

    assert!(parsed.show_hidden_files);
    assert_eq!(parsed.terminal_emulator, TerminalEmulator::Ghostty);
}

#[test]
fn loads_toml_config_before_legacy_config() {
    let temp_dir = tempfile::tempdir().expect("create temp config dir");
    fs::write(
        temp_dir.path().join("config.txt"),
        "show_hidden_files=true\nterminal_emulator=ghostty\n",
    )
    .expect("write legacy config");
    fs::write(
        temp_dir.path().join("config.toml"),
        "show_hidden_files = false\nterminal_emulator = \"kitty\"\n",
    )
    .expect("write toml config");

    let parsed = load_user_config_from_dir(temp_dir.path(), default_user_config());

    assert!(!parsed.show_hidden_files);
    assert_eq!(parsed.terminal_emulator, TerminalEmulator::Kitty);
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
