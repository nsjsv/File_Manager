use std::path::PathBuf;

use desktop_linux::{DisplayRendererGpu, TerminalEmulator};
use file_core::{FileOperationVerification, SortDirection, SortField};

use crate::matugen_theme::{
    default_custom_color_scheme, ColorSchemePreset, CustomColorScheme, ThemeMode,
};
use crate::model::{BrowserViewMode, ListDirectorySizeDisplayMode, SearchHistory};
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
const MATUGEN_THEME_FILE_NAME: &str = "matugen.toml";
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
pub(crate) const DEFAULT_PREVIEW_TEXT_SIZE_BYTES: u64 = 25 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const DEFAULT_PREVIEW_IMAGE_SIZE_BYTES: u64 = 100 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const DEFAULT_PREVIEW_VIDEO_SIZE_BYTES: u64 = 1024 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const DEFAULT_PREVIEW_AUDIO_SIZE_BYTES: u64 = 200 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const DEFAULT_PREVIEW_ARCHIVE_SIZE_BYTES: u64 = 25 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const DEFAULT_PREVIEW_DOCUMENT_SIZE_BYTES: u64 = 100 * PREVIEW_FILE_SIZE_UNIT_BYTES;
pub(crate) const MIN_PREVIEW_DIRECTORY_EXPAND_LEVELS: u8 = 0;
pub(crate) const MAX_PREVIEW_DIRECTORY_EXPAND_LEVELS: u8 = 3;
pub(crate) const DEFAULT_PREVIEW_DIRECTORY_EXPAND_LEVELS: u8 = 1;
pub(crate) const DEFAULT_SEARCH_MAX_EXTRACT_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const DEFAULT_ICON_GRID_SIZE: u32 = 96;
pub(crate) const MIN_ICON_GRID_SIZE: u32 = 64;
pub(crate) const MAX_ICON_GRID_SIZE: u32 = 192;
pub(crate) const ICON_GRID_SIZE_STEP: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiLanguage {
    English,
    Chinese,
}

impl UiLanguage {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            Self::English => 0,
            Self::Chinese => 1,
        }
    }

    pub(crate) const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Chinese,
            _ => Self::English,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UiLanguageSetting {
    System,
    English,
    Chinese,
}

impl UiLanguageSetting {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "system" => Some(Self::System),
            "english" => Some(Self::English),
            "chinese" => Some(Self::Chinese),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "english",
            Self::Chinese => "chinese",
        }
    }

    pub(crate) fn resolve(self, system_language: UiLanguage) -> UiLanguage {
        match self {
            Self::System => system_language,
            Self::English => UiLanguage::English,
            Self::Chinese => UiLanguage::Chinese,
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
        "icons" => Some(BrowserViewMode::Icons),
        _ => None,
    }
}

pub(crate) fn browser_view_mode_config_value(view_mode: BrowserViewMode) -> &'static str {
    match view_mode {
        BrowserViewMode::Columns => "columns",
        BrowserViewMode::List => "list",
        BrowserViewMode::Icons => "icons",
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

pub(crate) fn list_directory_size_display_mode_from_config_value(
    value: &str,
) -> Option<ListDirectorySizeDisplayMode> {
    ListDirectorySizeDisplayMode::from_config_value(value)
}

pub(crate) fn list_directory_size_display_mode_config_value(
    mode: ListDirectorySizeDisplayMode,
) -> &'static str {
    mode.config_value()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewFileSizeKind {
    Text,
    Image,
    Video,
    Audio,
    Archive,
    Document,
}

impl PreviewFileSizeKind {
    pub(crate) const ALL: [PreviewFileSizeKind; 6] = [
        Self::Text,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Archive,
        Self::Document,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewFileSizeLimits {
    pub(crate) text_bytes: u64,
    pub(crate) image_bytes: u64,
    pub(crate) video_bytes: u64,
    pub(crate) audio_bytes: u64,
    pub(crate) archive_bytes: u64,
    pub(crate) document_bytes: u64,
}

impl PreviewFileSizeLimits {
    pub(crate) fn with_default_limits() -> Self {
        Self {
            text_bytes: DEFAULT_PREVIEW_TEXT_SIZE_BYTES,
            image_bytes: DEFAULT_PREVIEW_IMAGE_SIZE_BYTES,
            video_bytes: DEFAULT_PREVIEW_VIDEO_SIZE_BYTES,
            audio_bytes: DEFAULT_PREVIEW_AUDIO_SIZE_BYTES,
            archive_bytes: DEFAULT_PREVIEW_ARCHIVE_SIZE_BYTES,
            document_bytes: DEFAULT_PREVIEW_DOCUMENT_SIZE_BYTES,
        }
    }

    pub(crate) fn limit(self, kind: PreviewFileSizeKind) -> u64 {
        match kind {
            PreviewFileSizeKind::Text => self.text_bytes,
            PreviewFileSizeKind::Image => self.image_bytes,
            PreviewFileSizeKind::Video => self.video_bytes,
            PreviewFileSizeKind::Audio => self.audio_bytes,
            PreviewFileSizeKind::Archive => self.archive_bytes,
            PreviewFileSizeKind::Document => self.document_bytes,
        }
    }

    pub(crate) fn set_limit(&mut self, kind: PreviewFileSizeKind, bytes: u64) {
        match kind {
            PreviewFileSizeKind::Text => self.text_bytes = bytes,
            PreviewFileSizeKind::Image => self.image_bytes = bytes,
            PreviewFileSizeKind::Video => self.video_bytes = bytes,
            PreviewFileSizeKind::Audio => self.audio_bytes = bytes,
            PreviewFileSizeKind::Archive => self.archive_bytes = bytes,
            PreviewFileSizeKind::Document => self.document_bytes = bytes,
        }
    }

    /// 迁移时用旧的全局单值上限同时填充全部六个类型。
    pub(crate) fn from_legacy_global_bytes(bytes: u64) -> Self {
        Self {
            text_bytes: bytes,
            image_bytes: bytes,
            video_bytes: bytes,
            audio_bytes: bytes,
            archive_bytes: bytes,
            document_bytes: bytes,
        }
    }
}

pub(crate) fn normalize_preview_directory_expand_levels(levels: u8) -> u8 {
    levels.clamp(
        MIN_PREVIEW_DIRECTORY_EXPAND_LEVELS,
        MAX_PREVIEW_DIRECTORY_EXPAND_LEVELS,
    )
}

pub(crate) fn preview_size_limit_mib(bytes: u64) -> u64 {
    bytes.div_ceil(PREVIEW_FILE_SIZE_UNIT_BYTES)
}

pub(crate) fn preview_size_limit_bytes_from_mib(mib: u64) -> Option<u64> {
    mib.checked_mul(PREVIEW_FILE_SIZE_UNIT_BYTES)
}

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) network_list_thumbnail_downloads_enabled: bool,
    pub(crate) preview_size_limits: PreviewFileSizeLimits,
    pub(crate) preview_directory_expand_levels: u8,
    pub(crate) show_hidden_files: bool,
    pub(crate) language_setting: UiLanguageSetting,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) network_connections: Vec<SavedNetworkConnection>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) rendering_gpu_preference: RenderingGpuPreference,
    pub(crate) search_content_indexing_enabled: bool,
    pub(crate) search_max_extract_bytes: u64,
    pub(crate) search_history: SearchHistory,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) color_scheme: ColorSchemePreset,
    pub(crate) custom_color_scheme: CustomColorScheme,
    pub(crate) file_operation_verification: FileOperationVerification,
    pub(crate) browser_view_mode: BrowserViewMode,
    pub(crate) window_controls: crate::model::WindowControlsConfig,
    pub(crate) icon_grid_size: u32,
    pub(crate) list_view_preferences: crate::model::ListViewPreferences,
    pub(crate) list_directory_size_display_mode: ListDirectorySizeDisplayMode,
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
    UserConfig {
        thumbnail_cache_dir: cache_base.join("thumbnails"),
        network_list_thumbnail_downloads_enabled: false,
        preview_size_limits: PreviewFileSizeLimits::with_default_limits(),
        preview_directory_expand_levels: DEFAULT_PREVIEW_DIRECTORY_EXPAND_LEVELS,
        show_hidden_files: false,
        language_setting: UiLanguageSetting::System,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        search_content_indexing_enabled: true,
        search_max_extract_bytes: DEFAULT_SEARCH_MAX_EXTRACT_BYTES,
        search_history: SearchHistory::default(),
        theme_mode: ThemeMode::Automatic,
        color_scheme: ColorSchemePreset::Default,
        custom_color_scheme: default_custom_color_scheme(),
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
        window_controls: crate::model::WindowControlsConfig::default(),
        icon_grid_size: DEFAULT_ICON_GRID_SIZE,
        list_view_preferences: crate::model::ListViewPreferences::default(),
        list_directory_size_display_mode: ListDirectorySizeDisplayMode::ItemCount,
        startup_location_policy: StartupLocationPolicy::Home,
        startup_custom_directory: fallback_base.clone(),
        save_view_state: false,
        shortcuts: ShortcutConfig::defaults(),
    }
}

pub(crate) fn ui_thread_startup_config() -> UserConfig {
    UserConfig {
        thumbnail_cache_dir: PathBuf::new(),
        network_list_thumbnail_downloads_enabled: false,
        preview_size_limits: PreviewFileSizeLimits::with_default_limits(),
        preview_directory_expand_levels: DEFAULT_PREVIEW_DIRECTORY_EXPAND_LEVELS,
        show_hidden_files: false,
        language_setting: UiLanguageSetting::System,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        search_content_indexing_enabled: true,
        search_max_extract_bytes: DEFAULT_SEARCH_MAX_EXTRACT_BYTES,
        search_history: SearchHistory::default(),
        theme_mode: ThemeMode::Automatic,
        color_scheme: ColorSchemePreset::Default,
        custom_color_scheme: default_custom_color_scheme(),
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
        window_controls: crate::model::WindowControlsConfig::default(),
        icon_grid_size: DEFAULT_ICON_GRID_SIZE,
        list_view_preferences: crate::model::ListViewPreferences::default(),
        list_directory_size_display_mode: ListDirectorySizeDisplayMode::ItemCount,
        startup_location_policy: StartupLocationPolicy::Home,
        startup_custom_directory: PathBuf::new(),
        save_view_state: false,
        shortcuts: ShortcutConfig::defaults(),
    }
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

pub(crate) fn normalize_icon_grid_size(size: u32) -> u32 {
    size.clamp(MIN_ICON_GRID_SIZE, MAX_ICON_GRID_SIZE)
}

pub(super) fn app_config_dir_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join(APP_DIR_NAME))
}

pub(crate) fn preview_size_limit_mib_inputs(limits: &PreviewFileSizeLimits) -> [String; 6] {
    PreviewFileSizeKind::ALL.map(|kind| preview_size_limit_mib(limits.limit(kind)).to_string())
}

pub(crate) fn matugen_theme_file_path() -> Option<PathBuf> {
    app_config_dir_path().map(|path| path.join(MATUGEN_THEME_FILE_NAME))
}

pub(crate) fn toml_string<'a>(document: &'a toml::Table, key: &str) -> Option<&'a str> {
    document
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests;
