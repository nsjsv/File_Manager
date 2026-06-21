use std::path::{Path, PathBuf};
use std::{fs, io};

use desktop_linux::{
    DisplayRendererGpu, NetworkConnection, NetworkConnectionId, NetworkProtocol, TerminalEmulator,
};
use file_core::FileOperationVerification;
use file_index::{default_search_index_exclude_patterns, DirectoryErrorPolicy};

use crate::model::BrowserViewMode;
use crate::shortcuts::ShortcutConfig;

const APP_DIR_NAME: &str = "file-manager";
const CONFIG_FILE_NAME: &str = "config.toml";
const STATE_DATABASE_FILE_NAME: &str = "state.sqlite";
const SEARCH_INDEX_DIR_KEY: &str = "search_index_dir";
const SEARCH_INDEX_EXCLUDE_PATTERNS_KEY: &str = "search_index_exclude_patterns";
const SEARCH_INDEX_CONTENT_ENABLED_KEY: &str = "search_index_content_enabled";
const SEARCH_INDEX_MEDIA_ENABLED_KEY: &str = "search_index_media_enabled";
const SEARCH_INDEX_DIRECTORY_ERROR_POLICY_KEY: &str = "search_index_directory_error_policy";
const SEARCH_MODE_KEY: &str = "search_mode";
const SEARCH_MODE_PROMPT_KEY: &str = "search_mode_prompt";
const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const SHOW_HIDDEN_FILES_KEY: &str = "show_hidden_files";
const SIDEBAR_WIDTH_KEY: &str = "sidebar_width";
const SIDEBAR_FAVORITES_KEY: &str = "sidebar_favorites";
const SIDEBAR_FAVORITE_LABEL_KEY: &str = "label";
const SIDEBAR_FAVORITE_PATH_KEY: &str = "path";
const NETWORK_CONNECTIONS_KEY: &str = "network_connections";
const NETWORK_CONNECTION_ID_KEY: &str = "id";
const NETWORK_CONNECTION_LABEL_KEY: &str = "label";
const NETWORK_CONNECTION_PROTOCOL_KEY: &str = "protocol";
const NETWORK_CONNECTION_URI_KEY: &str = "uri";
const TERMINAL_EMULATOR_KEY: &str = "terminal_emulator";
const RENDERING_BACKEND_KEY: &str = "rendering_backend";
const FILE_OPERATION_VERIFICATION_KEY: &str = "file_operation_verification";
const BROWSER_VIEW_MODE_KEY: &str = "browser_view_mode";
const SHORTCUTS_KEY: &str = "shortcuts";

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

pub(crate) fn file_operation_verification_label(
    verification: FileOperationVerification,
) -> &'static str {
    match verification {
        FileOperationVerification::BasicMetadata => "Basic + Metadata",
        FileOperationVerification::Strong => "Strong",
    }
}

pub(crate) fn file_operation_verification_description(
    verification: FileOperationVerification,
) -> &'static str {
    match verification {
        FileOperationVerification::BasicMetadata => {
            "Checks the copied target type, size, and key metadata after the transfer."
        }
        FileOperationVerification::Strong => {
            "Includes Basic + Metadata, then compares copied file content hashes."
        }
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

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) search_index_exclude_patterns: Vec<String>,
    pub(crate) search_index_content_enabled: bool,
    pub(crate) search_index_media_enabled: bool,
    pub(crate) search_index_directory_error_policy: DirectoryErrorPolicy,
    pub(crate) search_mode: SearchBackendMode,
    pub(crate) search_mode_prompt: SearchModePromptStatus,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) show_hidden_files: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) network_connections: Vec<NetworkConnection>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) rendering_gpu_preference: RenderingGpuPreference,
    pub(crate) file_operation_verification: FileOperationVerification,
    pub(crate) browser_view_mode: BrowserViewMode,
    pub(crate) shortcuts: ShortcutConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SidebarFavoriteConfig {
    pub(crate) label: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn load_user_config() -> UserConfig {
    let default = default_user_config();
    let Some(config_dir) = app_config_dir_path() else {
        return default;
    };

    load_user_config_from_dir(&config_dir, default)
}

pub(crate) fn save_user_config(config: &UserConfig) -> io::Result<()> {
    let config_file = config_file_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "user configuration directory is unavailable",
        )
    })?;
    write_user_config(&config_file, config)
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
    let cache_base = dirs::cache_dir()
        .unwrap_or(fallback_base)
        .join(APP_DIR_NAME);
    UserConfig {
        search_index_dir: cache_base.join("search-index"),
        search_index_exclude_patterns: default_search_index_exclude_patterns_config(),
        search_index_content_enabled: false,
        search_index_media_enabled: false,
        search_index_directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
        search_mode: SearchBackendMode::Simple,
        search_mode_prompt: SearchModePromptStatus::Pending,
        thumbnail_cache_dir: cache_base.join("thumbnails"),
        show_hidden_files: false,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
        shortcuts: ShortcutConfig::defaults(),
    }
}

pub(crate) fn ui_thread_startup_config() -> UserConfig {
    UserConfig {
        search_index_dir: PathBuf::new(),
        search_index_exclude_patterns: default_search_index_exclude_patterns_config(),
        search_index_content_enabled: false,
        search_index_media_enabled: false,
        search_index_directory_error_policy: DirectoryErrorPolicy::SkipUnreadable,
        search_mode: SearchBackendMode::Simple,
        search_mode_prompt: SearchModePromptStatus::Pending,
        thumbnail_cache_dir: PathBuf::new(),
        show_hidden_files: false,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        sidebar_favorites: None,
        network_connections: Vec::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
        file_operation_verification: DEFAULT_FILE_OPERATION_VERIFICATION,
        browser_view_mode: BrowserViewMode::Columns,
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

fn app_config_dir_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join(APP_DIR_NAME))
}

fn config_file_path() -> Option<PathBuf> {
    app_config_dir_path().map(|path| path.join(CONFIG_FILE_NAME))
}

fn load_user_config_from_dir(config_dir: &Path, default: UserConfig) -> UserConfig {
    let config_file = config_dir.join(CONFIG_FILE_NAME);
    match fs::read_to_string(&config_file) {
        Ok(content) => parse_toml_user_config(&content, default),
        Err(error) if error.kind() == io::ErrorKind::NotFound => default,
        Err(_) => default,
    }
}

fn parse_toml_user_config(content: &str, default: UserConfig) -> UserConfig {
    let Ok(document) = content.parse::<toml::Table>() else {
        return default;
    };

    let mut config = default;
    if let Some(value) = toml_string(&document, SEARCH_INDEX_DIR_KEY) {
        config.search_index_dir = PathBuf::from(value);
    }
    if let Some(patterns) = toml_string_array(&document, SEARCH_INDEX_EXCLUDE_PATTERNS_KEY) {
        config.search_index_exclude_patterns = normalize_search_index_exclude_patterns(patterns);
    }
    if let Some(value) = document
        .get(SEARCH_INDEX_CONTENT_ENABLED_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.search_index_content_enabled = value;
    }
    if let Some(value) = document
        .get(SEARCH_INDEX_MEDIA_ENABLED_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.search_index_media_enabled = value;
    }
    if let Some(value) = toml_string(&document, SEARCH_INDEX_DIRECTORY_ERROR_POLICY_KEY) {
        if let Some(policy) = DirectoryErrorPolicy::from_config_value(value) {
            config.search_index_directory_error_policy = policy;
        }
    }
    if let Some(value) = toml_string(&document, SEARCH_MODE_KEY) {
        if let Some(search_mode) = SearchBackendMode::from_config_value(value) {
            config.search_mode = search_mode;
        }
    }
    if let Some(value) = toml_string(&document, SEARCH_MODE_PROMPT_KEY) {
        if let Some(status) = SearchModePromptStatus::from_config_value(value) {
            config.search_mode_prompt = status;
        }
    }
    if let Some(value) = toml_string(&document, THUMBNAIL_CACHE_DIR_KEY) {
        config.thumbnail_cache_dir = PathBuf::from(value);
    }
    if let Some(value) = document
        .get(SHOW_HIDDEN_FILES_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.show_hidden_files = value;
    }
    if let Some(width) = document.get(SIDEBAR_WIDTH_KEY).and_then(toml_number_as_f32) {
        config.sidebar_width = normalize_sidebar_width(width);
    }
    if let Some(favorites) = parse_toml_sidebar_favorites(&document) {
        config.sidebar_favorites = Some(favorites);
    }
    config.network_connections = parse_toml_network_connections(&document);
    if let Some(value) = toml_string(&document, TERMINAL_EMULATOR_KEY) {
        if let Some(terminal_emulator) = TerminalEmulator::from_config_value(value) {
            config.terminal_emulator = terminal_emulator;
        }
    }
    if let Some(value) = toml_string(&document, RENDERING_BACKEND_KEY) {
        if let Some(preference) = RenderingGpuPreference::from_config_value(value) {
            config.rendering_gpu_preference = preference;
        }
    }
    if let Some(value) = toml_string(&document, FILE_OPERATION_VERIFICATION_KEY) {
        if let Some(verification) = file_operation_verification_from_config_value(value) {
            config.file_operation_verification = verification;
        }
    }
    if let Some(value) = toml_string(&document, BROWSER_VIEW_MODE_KEY) {
        if let Some(view_mode) = browser_view_mode_from_config_value(value) {
            config.browser_view_mode = view_mode;
        }
    }
    if let Some(table) = document.get(SHORTCUTS_KEY).and_then(toml::Value::as_table) {
        config.shortcuts.apply_toml_table(table);
    }
    config
}

fn toml_string<'a>(document: &'a toml::Table, key: &str) -> Option<&'a str> {
    document
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
}

fn toml_string_array(document: &toml::Table, key: &str) -> Option<Vec<String>> {
    let values = document.get(key)?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(toml::Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
    )
}

fn write_user_config(path: &Path, config: &UserConfig) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = config.search_index_dir.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(parent) = config.thumbnail_cache_dir.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let content = toml_user_config_content(config).map_err(io::Error::other)?;
    fs::write(path, content)
}

fn toml_user_config_content(config: &UserConfig) -> Result<String, toml::ser::Error> {
    let mut document = toml::Table::new();
    document.insert(
        SEARCH_INDEX_DIR_KEY.to_string(),
        toml::Value::String(config.search_index_dir.to_string_lossy().into_owned()),
    );
    document.insert(
        SEARCH_INDEX_EXCLUDE_PATTERNS_KEY.to_string(),
        toml::Value::Array(
            config
                .search_index_exclude_patterns
                .iter()
                .cloned()
                .map(toml::Value::String)
                .collect(),
        ),
    );
    document.insert(
        SEARCH_INDEX_CONTENT_ENABLED_KEY.to_string(),
        toml::Value::Boolean(config.search_index_content_enabled),
    );
    document.insert(
        SEARCH_INDEX_MEDIA_ENABLED_KEY.to_string(),
        toml::Value::Boolean(config.search_index_media_enabled),
    );
    document.insert(
        SEARCH_INDEX_DIRECTORY_ERROR_POLICY_KEY.to_string(),
        toml::Value::String(
            config
                .search_index_directory_error_policy
                .config_value()
                .to_string(),
        ),
    );
    document.insert(
        SEARCH_MODE_KEY.to_string(),
        toml::Value::String(config.search_mode.config_value().to_string()),
    );
    document.insert(
        SEARCH_MODE_PROMPT_KEY.to_string(),
        toml::Value::String(config.search_mode_prompt.config_value().to_string()),
    );
    document.insert(
        THUMBNAIL_CACHE_DIR_KEY.to_string(),
        toml::Value::String(config.thumbnail_cache_dir.to_string_lossy().into_owned()),
    );
    document.insert(
        SHOW_HIDDEN_FILES_KEY.to_string(),
        toml::Value::Boolean(config.show_hidden_files),
    );
    document.insert(
        SIDEBAR_WIDTH_KEY.to_string(),
        toml::Value::Float(normalize_sidebar_width(config.sidebar_width) as f64),
    );
    document.insert(
        TERMINAL_EMULATOR_KEY.to_string(),
        toml::Value::String(config.terminal_emulator.config_value().to_string()),
    );
    document.insert(
        RENDERING_BACKEND_KEY.to_string(),
        toml::Value::String(config.rendering_gpu_preference.config_value().to_string()),
    );
    document.insert(
        FILE_OPERATION_VERIFICATION_KEY.to_string(),
        toml::Value::String(
            file_operation_verification_config_value(config.file_operation_verification)
                .to_string(),
        ),
    );
    document.insert(
        BROWSER_VIEW_MODE_KEY.to_string(),
        toml::Value::String(browser_view_mode_config_value(config.browser_view_mode).to_string()),
    );
    document.insert(
        SHORTCUTS_KEY.to_string(),
        toml::Value::Table(config.shortcuts.toml_table()),
    );
    if let Some(favorites) = &config.sidebar_favorites {
        document.insert(
            SIDEBAR_FAVORITES_KEY.to_string(),
            toml::Value::Array(toml_sidebar_favorite_values(favorites)),
        );
    }
    document.insert(
        NETWORK_CONNECTIONS_KEY.to_string(),
        toml::Value::Array(toml_network_connection_values(&config.network_connections)),
    );

    let content = toml::to_string_pretty(&document)?;
    Ok(format!("# File Manager user configuration\n{content}"))
}

fn parse_toml_sidebar_favorites(document: &toml::Table) -> Option<Vec<SidebarFavoriteConfig>> {
    let entries = document.get(SIDEBAR_FAVORITES_KEY)?.as_array()?;
    let mut favorites = Vec::new();
    for entry in entries {
        let Some(table) = entry.as_table() else {
            continue;
        };
        let Some(path) = toml_string(table, SIDEBAR_FAVORITE_PATH_KEY) else {
            continue;
        };
        let path = PathBuf::from(path);
        if path.as_os_str().is_empty() {
            continue;
        }
        let label = toml_string(table, SIDEBAR_FAVORITE_LABEL_KEY)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| sidebar_favorite_label_from_path(&path));
        favorites.push(SidebarFavoriteConfig { label, path });
    }
    Some(favorites)
}

fn toml_sidebar_favorite_values(favorites: &[SidebarFavoriteConfig]) -> Vec<toml::Value> {
    favorites
        .iter()
        .map(|favorite| {
            let mut table = toml::Table::new();
            table.insert(
                SIDEBAR_FAVORITE_LABEL_KEY.to_string(),
                toml::Value::String(favorite.label.clone()),
            );
            table.insert(
                SIDEBAR_FAVORITE_PATH_KEY.to_string(),
                toml::Value::String(favorite.path.to_string_lossy().into_owned()),
            );
            toml::Value::Table(table)
        })
        .collect()
}

fn parse_toml_network_connections(document: &toml::Table) -> Vec<NetworkConnection> {
    let Some(entries) = document
        .get(NETWORK_CONNECTIONS_KEY)
        .and_then(toml::Value::as_array)
    else {
        return Vec::new();
    };

    let mut connections = Vec::new();
    for entry in entries {
        let Some(table) = entry.as_table() else {
            continue;
        };
        let Some(id) = toml_string(table, NETWORK_CONNECTION_ID_KEY) else {
            continue;
        };
        let Some(protocol) = toml_string(table, NETWORK_CONNECTION_PROTOCOL_KEY)
            .and_then(NetworkProtocol::from_config_value)
        else {
            continue;
        };
        let Some(uri) = toml_string(table, NETWORK_CONNECTION_URI_KEY) else {
            continue;
        };
        let label = table
            .get(NETWORK_CONNECTION_LABEL_KEY)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        if id.trim().is_empty() {
            continue;
        }
        if let Ok(connection) =
            NetworkConnection::new(NetworkConnectionId::new(id.trim()), label, protocol, uri)
        {
            connections.push(connection);
        }
    }
    connections
}

fn toml_network_connection_values(connections: &[NetworkConnection]) -> Vec<toml::Value> {
    connections
        .iter()
        .map(|connection| {
            let mut table = toml::Table::new();
            table.insert(
                NETWORK_CONNECTION_ID_KEY.to_string(),
                toml::Value::String(connection.id.as_str().to_owned()),
            );
            table.insert(
                NETWORK_CONNECTION_LABEL_KEY.to_string(),
                toml::Value::String(connection.label.clone()),
            );
            table.insert(
                NETWORK_CONNECTION_PROTOCOL_KEY.to_string(),
                toml::Value::String(connection.protocol.config_value().to_owned()),
            );
            table.insert(
                NETWORK_CONNECTION_URI_KEY.to_string(),
                toml::Value::String(connection.uri.clone()),
            );
            toml::Value::Table(table)
        })
        .collect()
}

fn sidebar_favorite_label_from_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn toml_number_as_f32(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Float(value) => Some(*value as f32),
        toml::Value::Integer(value) => Some(*value as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
