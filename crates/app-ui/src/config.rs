use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

use desktop_linux::TerminalEmulator;

const APP_DIR_NAME: &str = "file-manager";
const CONFIG_FILE_NAME: &str = "config.txt";
const STATE_DATABASE_FILE_NAME: &str = "state.sqlite";
const SEARCH_INDEX_DIR_KEY: &str = "search_index_dir";
const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const SHOW_HIDDEN_FILES_KEY: &str = "show_hidden_files";
const COLUMN_WIDTH_OVERRIDES_KEY: &str = "column_width_overrides";
const TERMINAL_EMULATOR_KEY: &str = "terminal_emulator";
const RENDERING_BACKEND_KEY: &str = "rendering_backend";

pub(crate) const DEFAULT_TERMINAL_EMULATOR: TerminalEmulator = TerminalEmulator::Automatic;
pub(crate) const DEFAULT_RENDERING_BACKEND_PREFERENCE: RenderingBackendPreference =
    RenderingBackendPreference::Software;
pub(crate) const MIN_COLUMN_WIDTH: f32 = 96.0;
pub(crate) const MAX_COLUMN_WIDTH: f32 = 960.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RenderingBackendPreference {
    Software,
    Gpu,
}

impl RenderingBackendPreference {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "software" => Some(Self::Software),
            "gpu" => Some(Self::Gpu),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Software => "software",
            Self::Gpu => "gpu",
        }
    }

    pub(crate) fn iced_backend_candidates(self) -> &'static str {
        match self {
            Self::Software => "tiny-skia",
            Self::Gpu => "wgpu,tiny-skia",
        }
    }

    pub(crate) fn wgpu_power_preference(self) -> Option<&'static str> {
        match self {
            Self::Software => None,
            Self::Gpu => Some("high"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) show_hidden_files: bool,
    pub(crate) column_width_overrides: HashMap<usize, f32>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) rendering_backend_preference: RenderingBackendPreference,
}

pub(crate) fn load_user_config() -> UserConfig {
    let default = default_user_config();
    let Some(config_file) = config_file_path() else {
        return default;
    };

    match fs::read_to_string(&config_file) {
        Ok(content) => parse_user_config(&content, default),
        Err(error) if error.kind() == io::ErrorKind::NotFound => default,
        Err(_) => default,
    }
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
        column_width_overrides: HashMap::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_backend_preference: DEFAULT_RENDERING_BACKEND_PREFERENCE,
    }
}

pub(crate) fn ui_thread_startup_config() -> UserConfig {
    UserConfig {
        search_index_dir: PathBuf::new(),
        thumbnail_cache_dir: PathBuf::new(),
        show_hidden_files: false,
        column_width_overrides: HashMap::new(),
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
        rendering_backend_preference: DEFAULT_RENDERING_BACKEND_PREFERENCE,
    }
}

pub(crate) fn normalize_column_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
    } else {
        MIN_COLUMN_WIDTH
    }
}

fn config_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join(APP_DIR_NAME).join(CONFIG_FILE_NAME))
}

fn parse_user_config(content: &str, default: UserConfig) -> UserConfig {
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
            COLUMN_WIDTH_OVERRIDES_KEY => {
                config.column_width_overrides = parse_column_width_overrides(value);
            }
            TERMINAL_EMULATOR_KEY => {
                if let Some(terminal_emulator) = TerminalEmulator::from_config_value(value) {
                    config.terminal_emulator = terminal_emulator;
                }
            }
            RENDERING_BACKEND_KEY => {
                if let Some(preference) = RenderingBackendPreference::from_config_value(value) {
                    config.rendering_backend_preference = preference;
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
    fs::write(
        path,
        format!(
            "# File Manager user configuration\n{}={}\n{}={}\n{}={}\n{}={}\n{}={}\n{}={}\n",
            SEARCH_INDEX_DIR_KEY,
            config.search_index_dir.to_string_lossy(),
            THUMBNAIL_CACHE_DIR_KEY,
            config.thumbnail_cache_dir.to_string_lossy(),
            SHOW_HIDDEN_FILES_KEY,
            config.show_hidden_files,
            COLUMN_WIDTH_OVERRIDES_KEY,
            column_width_overrides_value(&config.column_width_overrides),
            TERMINAL_EMULATOR_KEY,
            config.terminal_emulator.config_value(),
            RENDERING_BACKEND_KEY,
            config.rendering_backend_preference.config_value()
        ),
    )
}

fn parse_column_width_overrides(value: &str) -> HashMap<usize, f32> {
    let mut overrides = HashMap::new();
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
            overrides.insert(index, normalize_column_width(width));
        }
    }
    overrides
}

fn column_width_overrides_value(overrides: &HashMap<usize, f32>) -> String {
    let mut pairs = overrides.iter().collect::<Vec<_>>();
    pairs.sort_by_key(|(index, _)| **index);
    pairs
        .into_iter()
        .map(|(index, width)| format!("{index}:{width}"))
        .collect::<Vec<_>>()
        .join(",")
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
    fn parses_column_width_overrides() {
        let parsed = parse_user_config(
            "show_hidden_files=true\ncolumn_width_overrides=0:240.5,2:360\nterminal_emulator=ghostty\nrendering_backend=gpu\n",
            default_user_config(),
        );

        assert!(parsed.show_hidden_files);
        assert_eq!(parsed.column_width_overrides.get(&0), Some(&240.5));
        assert_eq!(parsed.column_width_overrides.get(&2), Some(&360.0));
        assert_eq!(parsed.terminal_emulator, TerminalEmulator::Ghostty);
        assert_eq!(
            parsed.rendering_backend_preference,
            RenderingBackendPreference::Gpu
        );
    }

    #[test]
    fn invalid_column_width_overrides_fall_back_to_empty() {
        let default = default_user_config();
        let parsed = parse_user_config(
            "show_hidden_files=maybe\ncolumn_width_overrides=bad,1:nan\nterminal_emulator=missing\nrendering_backend=metal\n",
            default.clone(),
        );

        assert_eq!(parsed.show_hidden_files, default.show_hidden_files);
        assert!(parsed.column_width_overrides.is_empty());
        assert_eq!(parsed.terminal_emulator, DEFAULT_TERMINAL_EMULATOR);
        assert_eq!(
            parsed.rendering_backend_preference,
            DEFAULT_RENDERING_BACKEND_PREFERENCE
        );
    }

    #[test]
    fn writes_rendering_backend_preference() {
        let temp_dir = tempfile::tempdir().expect("create temp config dir");
        let path = temp_dir.path().join("config.txt");
        let mut config = default_user_config();
        config.rendering_backend_preference = RenderingBackendPreference::Gpu;
        config.column_width_overrides.insert(2, 360.0);
        config.column_width_overrides.insert(0, 240.0);

        write_user_config(&path, &config).expect("write user config");

        let content = fs::read_to_string(path).expect("read user config");
        assert!(content.contains("column_width_overrides=0:240,2:360\n"));
        assert!(content.contains("rendering_backend=gpu\n"));
    }

    #[test]
    fn maps_rendering_backend_preferences_to_iced_candidates() {
        assert_eq!(
            RenderingBackendPreference::Software.iced_backend_candidates(),
            "tiny-skia"
        );
        assert_eq!(
            RenderingBackendPreference::Gpu.iced_backend_candidates(),
            "wgpu,tiny-skia"
        );
    }

    #[test]
    fn maps_gpu_rendering_to_high_power_wgpu_preference() {
        assert_eq!(
            RenderingBackendPreference::Software.wgpu_power_preference(),
            None
        );
        assert_eq!(
            RenderingBackendPreference::Gpu.wgpu_power_preference(),
            Some("high")
        );
    }
}
