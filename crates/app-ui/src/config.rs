use std::path::PathBuf;

use desktop_linux::{DisplayRendererGpu, TerminalEmulator};
use file_core::{FileOperationVerification, SortDirection, SortField};
use file_index::{default_search_index_exclude_patterns, DirectoryErrorPolicy, MediaMetadataScope};

use crate::model::BrowserViewMode;
use crate::network_connections::SavedNetworkConnection;
use crate::shortcuts::ShortcutConfig;

mod app_config;
pub(crate) use app_config::{load_app_config, save_app_config, AppConfig};
mod legacy_toml;
pub(crate) mod startup;
pub(crate) use startup::StartupLocationPolicy;
mod user_preferences;
pub(crate) use user_preferences::{
    load_user_config_for_app_config, save_user_preferences, UserPreferences,
};

const APP_DIR_NAME: &str = "file-manager";
pub(super) const CONFIG_FILE_NAME: &str = "config.toml";
const STATE_DATABASE_FILE_NAME: &str = "state.sqlite";

pub(crate) const DEFAULT_TERMINAL_EMULATOR: TerminalEmulator = TerminalEmulator::Automatic;
pub(crate) const DEFAULT_RENDERING_GPU_PREFERENCE: RenderingGpuPreference =
    RenderingGpuPreference::DisplayGpu;
pub(crate) const DEFAULT_FILE_OPERATION_VERIFICATION: FileOperationVerification =
    FileOperationVerification::BasicMetadata;
pub(crate) const DEFAULT_SIDEBAR_WIDTH: f32 = 180.0;
pub(crate) const MIN_SIDEBAR_WIDTH: f32 = 140.0;
pub(crate) const MAX_SIDEBAR_WIDTH: f32 = 360.0;
pub(crate) const MIN_COLUMN_WIDTH: f32 = 96.0;
pub(crate) const MAX_COLUMN_WIDTH: f32 = 960.0;
pub(crate) const PREVIEW_FILE_SIZE_UNIT_BYTES: u64 = 1024 * 1024;
pub(crate) const DEFAULT_MAX_PREVIEW_FILE_BYTES: u64 = 3 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchBackendMode {
    Simple,
    Indexed,
}

impl SearchBackendMode {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "simple" => Some(Self::Simple),
            "indexed" => Some(Self::Indexed),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Indexed => "indexed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchModePromptStatus {
    Pending,
    Completed,
}

impl SearchModePromptStatus {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderingGpuPreference {
    DisplayGpu,
    HighPerformanceGpu,
}

impl RenderingGpuPreference {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "display" => Some(Self::DisplayGpu),
            "gpu" => Some(Self::HighPerformanceGpu),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::DisplayGpu => "display",
            Self::HighPerformanceGpu => "gpu",
        }
    }

    pub(crate) fn iced_backend_candidates(self) -> &'static str {
        match self {
            Self::DisplayGpu | Self::HighPerformanceGpu => "wgpu",
        }
    }

    pub(crate) fn wgpu_power_preference(
        self,
        display_renderer_gpu: Option<&DisplayRendererGpu>,
    ) -> Option<&'static str> {
        match self {
            Self::DisplayGpu => Some(
                display_renderer_gpu
                    .map(|gpu| gpu.class().wgpu_power_preference())
                    .unwrap_or("none"),
            ),
            Self::HighPerformanceGpu => Some("high"),
        }
    }

    pub(crate) fn mesa_vulkan_device_select(
        self,
        display_renderer_gpu: Option<&DisplayRendererGpu>,
    ) -> Option<String> {
        match self {
            Self::DisplayGpu => {
                display_renderer_gpu.map(DisplayRendererGpu::mesa_vulkan_device_select)
            }
            Self::HighPerformanceGpu => None,
        }
    }
}

pub(crate) fn file_operation_verification_from_config_value(
    value: &str,
) -> Option<FileOperationVerification> {
    match value {
        "basic_metadata" => Some(FileOperationVerification::BasicMetadata),
        "strong" => Some(FileOperationVerification::Strong),
        _ => None,
    }
}

pub(crate) fn file_operation_verification_config_value(
    verification: FileOperationVerification,
) -> &'static str {
    match verification {
        FileOperationVerification::BasicMetadata => "basic_metadata",
        FileOperationVerification::Strong => "strong",
    }
}

pub(crate) fn browser_view_mode_from_config_value(value: &str) -> Option<BrowserViewMode> {
    match value {
        "columns" => Some(BrowserViewMode::Columns),
        "list" => Some(BrowserViewMode::List),
        _ => None,
    }
}

pub(crate) fn browser_view_mode_config_value(view_mode: BrowserViewMode) -> &'static str {
    match view_mode {
        BrowserViewMode::Columns => "columns",
        BrowserViewMode::List => "list",
    }
}

pub(crate) fn sort_field_from_config_value(value: &str) -> Option<SortField> {
    match value {
        "name" => Some(SortField::Name),
        "modified" => Some(SortField::Modified),
        "size" => Some(SortField::Size),
        "kind" => Some(SortField::Kind),
        _ => None,
    }
}

pub(crate) fn sort_field_config_value(field: SortField) -> &'static str {
    match field {
        SortField::Name => "name",
        SortField::Modified => "modified",
        SortField::Size => "size",
        SortField::Kind => "kind",
    }
}

pub(crate) fn sort_direction_from_config_value(value: &str) -> Option<SortDirection> {
    match value {
        "ascending" => Some(SortDirection::Ascending),
        "descending" => Some(SortDirection::Descending),
        _ => None,
    }
}

pub(crate) fn sort_direction_config_value(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "ascending",
        SortDirection::Descending => "descending",
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) search_index_exclude_patterns: Vec<String>,
    pub(crate) search_index_content_enabled: bool,
    pub(crate) search_index_media_scope: MediaMetadataScope,
    pub(crate) search_index_directory_error_policy: DirectoryErrorPolicy,
    pub(crate) search_mode: SearchBackendMode,
    pub(crate) search_mode_prompt: SearchModePromptStatus,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) network_list_thumbnail_downloads_enabled: bool,
    pub(crate) max_preview_file_bytes: u64,
    pub(crate) show_hidden_files: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) network_connections: Vec<SavedNetworkConnection>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) rendering_gpu_preference: RenderingGpuPreference,
    pub(crate) file_operation_verification: FileOperationVerification,
    pub(crate) browser_view_mode: BrowserViewMode,
    pub(crate) list_view_preferences: crate::model::ListViewPreferences,
    pub(crate) startup_location_policy: StartupLocationPolicy,
    pub(crate) startup_custom_directory: PathBuf,
    pub(crate) save_view_state: bool,
    pub(crate) shortcuts: ShortcutConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarFavoriteConfig {
    pub(crate) label: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn default_state_database_path() -> PathBuf {
    let fallback_base = dirs::home_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    dirs::data_dir()
        .unwrap_or(fallback_base)
        .join(APP_DIR_NAME)
        .join(STATE_DATABASE_FILE_NAME)
}

pub(crate) fn default_user_config() -> UserConfig {
    let fallback_base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let cache_base = dirs::cache_dir().unwrap_or_else(|| fallback_base.clone());
    let app_cache_base = cache_base.join(APP_DIR_NAME);
    UserConfig {
        search_index_dir: app_cache_base.join("search-index"),
        search_index_exclude_patterns: default_search_index_exclude_patterns_config(),
        search_index_content_enabled: false,
        search_index_media_scope: MediaMetadataScope::Off,
        search_index_directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
        search_mode: SearchBackendMode::Simple,
        search_mode_prompt: SearchModePromptStatus::Pending,
        thumbnail_cache_dir: cache_base.join("thumbnails"),
        network_list_thumbnail_downloads_enabled: false,
        max_preview_file_bytes: DEFAULT_MAX_PREVIEW_FILE_BYTES,
        show_hidden_files: false,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
        list_view_preferences: crate::model::ListViewPreferences::default(),
        startup_location_policy: StartupLocationPolicy::Home,
        startup_custom_directory: fallback_base.clone(),
        save_view_state: false,
        shortcuts: ShortcutConfig::defaults(),
    }
}

pub(crate) fn ui_thread_startup_config() -> UserConfig {
    UserConfig {
        search_index_dir: PathBuf::new(),
        search_index_exclude_patterns: default_search_index_exclude_patterns_config(),
        search_index_content_enabled: false,
        search_index_media_scope: MediaMetadataScope::Off,
        search_index_directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
        search_mode: SearchBackendMode::Simple,
        search_mode_prompt: SearchModePromptStatus::Pending,
        thumbnail_cache_dir: PathBuf::new(),
        network_list_thumbnail_downloads_enabled: false,
        max_preview_file_bytes: DEFAULT_MAX_PREVIEW_FILE_BYTES,
        show_hidden_files: false,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
        list_view_preferences: crate::model::ListViewPreferences::default(),
        startup_location_policy: StartupLocationPolicy::Home,
        startup_custom_directory: PathBuf::new(),
        save_view_state: false,
        shortcuts: ShortcutConfig::defaults(),
    }
}

fn default_search_index_exclude_patterns_config() -> Vec<String> {
    default_search_index_exclude_patterns()
        .iter()
        .map(|pattern| (*pattern).to_owned())
        .collect()
}

pub(crate) fn normalize_search_index_exclude_patterns(patterns: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for pattern in patterns
        .into_iter()
        .map(|pattern| pattern.trim().to_owned())
    {
        if !pattern.is_empty() && !normalized.contains(&pattern) {
            normalized.push(pattern);
        }
    }
    normalized
}

pub(crate) fn normalize_column_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
    } else {
        MIN_COLUMN_WIDTH
    }
}

pub(crate) fn normalize_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
    } else {
        DEFAULT_SIDEBAR_WIDTH
    }
}

pub(crate) fn normalize_max_preview_file_bytes(bytes: u64) -> u64 {
    bytes.max(1)
}

pub(crate) fn max_preview_file_mib(bytes: u64) -> u64 {
    normalize_max_preview_file_bytes(bytes).div_ceil(PREVIEW_FILE_SIZE_UNIT_BYTES)
}

pub(crate) fn max_preview_file_bytes_from_mib(mib: u64) -> Option<u64> {
    mib.checked_mul(PREVIEW_FILE_SIZE_UNIT_BYTES)
        .map(normalize_max_preview_file_bytes)
}

pub(super) fn app_config_dir_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join(APP_DIR_NAME))
}

pub(crate) fn toml_string<'a>(document: &'a toml::Table, key: &str) -> Option<&'a str> {
    document
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
