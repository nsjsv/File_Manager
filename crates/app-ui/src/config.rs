use std::collections::HashMap;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[cfg(test)]
use desktop_linux::DisplayRendererGpuClass;
use desktop_linux::{DisplayRendererGpu, TerminalEmulator};

const APP_DIR_NAME: &str = "file-manager";
const CONFIG_FILE_NAME: &str = "config.toml";
const LEGACY_CONFIG_FILE_NAME: &str = "config.txt";
const STATE_DATABASE_FILE_NAME: &str = "state.sqlite";
const SEARCH_INDEX_DIR_KEY: &str = "search_index_dir";
const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const SHOW_HIDDEN_FILES_KEY: &str = "show_hidden_files";
const STARTUP_INDEX_PROMPT_KEY: &str = "startup_index_prompt";
const SIDEBAR_WIDTH_KEY: &str = "sidebar_width";
const COLUMN_WIDTH_OVERRIDES_KEY: &str = "column_width_overrides";
const SIDEBAR_FAVORITES_KEY: &str = "sidebar_favorites";
const SIDEBAR_FAVORITE_LABEL_KEY: &str = "label";
const SIDEBAR_FAVORITE_PATH_KEY: &str = "path";
const TERMINAL_EMULATOR_KEY: &str = "terminal_emulator";
const RENDERING_BACKEND_KEY: &str = "rendering_backend";

pub(crate) const DEFAULT_TERMINAL_EMULATOR: TerminalEmulator = TerminalEmulator::Automatic;
pub(crate) const DEFAULT_RENDERING_GPU_PREFERENCE: RenderingGpuPreference =
    RenderingGpuPreference::DisplayGpu;
pub(crate) const DEFAULT_SIDEBAR_WIDTH: f32 = 180.0;
pub(crate) const MIN_SIDEBAR_WIDTH: f32 = 140.0;
pub(crate) const MAX_SIDEBAR_WIDTH: f32 = 360.0;
pub(crate) const MIN_COLUMN_WIDTH: f32 = 96.0;
pub(crate) const MAX_COLUMN_WIDTH: f32 = 960.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupIndexPromptStatus {
    Pending,
    Completed,
}

impl StartupIndexPromptStatus {
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
            "display" | "software" => Some(Self::DisplayGpu),
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

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) show_hidden_files: bool,
    pub(crate) startup_index_prompt: StartupIndexPromptStatus,
    pub(crate) sidebar_width: f32,
    pub(crate) legacy_column_width_overrides: HashMap<usize, f32>,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) rendering_gpu_preference: RenderingGpuPreference,
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

pub(crate) fn search_index_dir_for_root(base_dir: &Path, root: &Path) -> PathBuf {
    base_dir.join(path_hash(root))
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
        thumbnail_cache_dir: cache_base.join("thumbnails"),
        show_hidden_files: false,
        startup_index_prompt: StartupIndexPromptStatus::Pending,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        legacy_column_width_overrides: HashMap::new(),
        sidebar_favorites: None,
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
    }
}

pub(crate) fn ui_thread_startup_config() -> UserConfig {
    UserConfig {
        search_index_dir: PathBuf::new(),
        thumbnail_cache_dir: PathBuf::new(),
        show_hidden_files: false,
        startup_index_prompt: StartupIndexPromptStatus::Pending,
        sidebar_width: DEFAULT_SIDEBAR_WIDTH,
        legacy_column_width_overrides: HashMap::new(),
        sidebar_favorites: None,
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_gpu_preference: DEFAULT_RENDERING_GPU_PREFERENCE,
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
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let legacy_config_file = config_dir.join(LEGACY_CONFIG_FILE_NAME);
            match fs::read_to_string(&legacy_config_file) {
                Ok(content) => parse_legacy_user_config(&content, default),
                Err(_) => default,
            }
        }
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
    if let Some(value) = toml_string(&document, THUMBNAIL_CACHE_DIR_KEY) {
        config.thumbnail_cache_dir = PathBuf::from(value);
    }
    if let Some(value) = document
        .get(SHOW_HIDDEN_FILES_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.show_hidden_files = value;
    }
    if let Some(value) = toml_string(&document, STARTUP_INDEX_PROMPT_KEY) {
        if let Some(status) = StartupIndexPromptStatus::from_config_value(value) {
            config.startup_index_prompt = status;
        }
    }
    if let Some(width) = document.get(SIDEBAR_WIDTH_KEY).and_then(toml_number_as_f32) {
        config.sidebar_width = normalize_sidebar_width(width);
    }
    if let Some(table) = document
        .get(COLUMN_WIDTH_OVERRIDES_KEY)
        .and_then(toml::Value::as_table)
    {
        config.legacy_column_width_overrides = parse_toml_column_width_overrides(table);
    }
    if let Some(favorites) = parse_toml_sidebar_favorites(&document) {
        config.sidebar_favorites = Some(favorites);
    }
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
    config
}

fn toml_string<'a>(document: &'a toml::Table, key: &str) -> Option<&'a str> {
    document
        .get(key)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
}

fn parse_legacy_user_config(content: &str, default: UserConfig) -> UserConfig {
    let mut config = default;
    for line in content.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            SEARCH_INDEX_DIR_KEY => config.search_index_dir = PathBuf::from(value),
            THUMBNAIL_CACHE_DIR_KEY => config.thumbnail_cache_dir = PathBuf::from(value),
            SHOW_HIDDEN_FILES_KEY => match value {
                "true" => config.show_hidden_files = true,
                "false" => config.show_hidden_files = false,
                _ => {}
            },
            STARTUP_INDEX_PROMPT_KEY => {
                if let Some(status) = StartupIndexPromptStatus::from_config_value(value) {
                    config.startup_index_prompt = status;
                }
            }
            SIDEBAR_WIDTH_KEY => {
                if let Ok(width) = value.parse::<f32>() {
                    config.sidebar_width = normalize_sidebar_width(width);
                }
            }
            COLUMN_WIDTH_OVERRIDES_KEY => {
                config.legacy_column_width_overrides = parse_legacy_column_width_overrides(value);
            }
            SIDEBAR_FAVORITES_KEY => {}
            TERMINAL_EMULATOR_KEY => {
                if let Some(terminal_emulator) = TerminalEmulator::from_config_value(value) {
                    config.terminal_emulator = terminal_emulator;
                }
            }
            RENDERING_BACKEND_KEY => {
                if let Some(preference) = RenderingGpuPreference::from_config_value(value) {
                    config.rendering_gpu_preference = preference;
                }
            }
            _ => {}
        }
    }
    config
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
        THUMBNAIL_CACHE_DIR_KEY.to_string(),
        toml::Value::String(config.thumbnail_cache_dir.to_string_lossy().into_owned()),
    );
    document.insert(
        SHOW_HIDDEN_FILES_KEY.to_string(),
        toml::Value::Boolean(config.show_hidden_files),
    );
    document.insert(
        STARTUP_INDEX_PROMPT_KEY.to_string(),
        toml::Value::String(config.startup_index_prompt.config_value().to_string()),
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
    if let Some(favorites) = &config.sidebar_favorites {
        document.insert(
            SIDEBAR_FAVORITES_KEY.to_string(),
            toml::Value::Array(toml_sidebar_favorite_values(favorites)),
        );
    }

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

fn sidebar_favorite_label_from_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn parse_toml_column_width_overrides(table: &toml::Table) -> HashMap<usize, f32> {
    let mut widths = HashMap::new();
    for (index, width) in table {
        let (Ok(index), Some(width)) = (index.parse::<usize>(), toml_number_as_f32(width)) else {
            continue;
        };
        if width.is_finite() {
            widths.insert(index, normalize_column_width(width));
        }
    }
    widths
}

fn toml_number_as_f32(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Float(value) => Some(*value as f32),
        toml::Value::Integer(value) => Some(*value as f32),
        _ => None,
    }
}

fn parse_legacy_column_width_overrides(value: &str) -> HashMap<usize, f32> {
    let mut widths = HashMap::new();
    for entry in value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let Some((index, width)) = entry.split_once(':') else {
            continue;
        };
        let (Ok(index), Ok(width)) = (index.trim().parse::<usize>(), width.trim().parse::<f32>())
        else {
            continue;
        };
        if width.is_finite() {
            widths.insert(index, normalize_column_width(width));
        }
    }
    widths
}

fn path_hash(path: &Path) -> String {
    #[cfg(unix)]
    {
        hash_bytes(path.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        hash_bytes(path.to_string_lossy().as_bytes())
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn parses_legacy_user_config() {
        let parsed = parse_legacy_user_config(
            "show_hidden_files=true\nstartup_index_prompt=completed\nsidebar_width=260.5\ncolumn_width_overrides=0:240.5,2:360\nterminal_emulator=ghostty\nrendering_backend=gpu\n",
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
        let parsed =
            parse_toml_user_config("rendering_backend = \"display\"\n", default_user_config());

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
        assert!(!content.contains("[column_width_overrides]"));

        let parsed = parse_toml_user_config(&content, default_user_config());
        assert!(parsed.legacy_column_width_overrides.is_empty());
        assert_eq!(
            parsed.rendering_gpu_preference,
            RenderingGpuPreference::HighPerformanceGpu
        );
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
        let integrated_gpu = DisplayRendererGpu::from_drm_ids(
            DisplayRendererGpuClass::Integrated,
            "0x1002",
            "0x15bf",
        );
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
        let integrated_gpu = DisplayRendererGpu::from_drm_ids(
            DisplayRendererGpuClass::Integrated,
            "0x1002",
            "0x15bf",
        );

        assert_eq!(
            RenderingGpuPreference::DisplayGpu.mesa_vulkan_device_select(Some(&integrated_gpu)),
            Some(String::from("1002:15bf!"))
        );
        assert_eq!(
            RenderingGpuPreference::DisplayGpu.mesa_vulkan_device_select(None),
            None
        );
        assert_eq!(
            RenderingGpuPreference::HighPerformanceGpu
                .mesa_vulkan_device_select(Some(&integrated_gpu)),
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
}
