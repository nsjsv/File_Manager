#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{fs, io};

use desktop_linux::TerminalEmulator;

use crate::model::ColumnViewMode;

const APP_DIR_NAME: &str = "file-manager";
const CONFIG_FILE_NAME: &str = "config.txt";
const STATE_DATABASE_FILE_NAME: &str = "state.sqlite";
const SEARCH_INDEX_DIR_KEY: &str = "search_index_dir";
const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const SHOW_HIDDEN_FILES_KEY: &str = "show_hidden_files";
const COLUMN_VIEW_MODE_KEY: &str = "column_view_mode";
const COLUMN_FIXED_COUNT_KEY: &str = "column_fixed_count";
const UNBOUNDED_COLUMN_WIDTH_KEY: &str = "unbounded_column_width";
const TERMINAL_EMULATOR_KEY: &str = "terminal_emulator";

pub(crate) const DEFAULT_COLUMN_VIEW_MODE: ColumnViewMode = ColumnViewMode::Unbounded;
pub(crate) const DEFAULT_COLUMN_FIXED_COUNT: usize = 3;
pub(crate) const DEFAULT_UNBOUNDED_COLUMN_WIDTH: f32 = 260.0;
pub(crate) const DEFAULT_TERMINAL_EMULATOR: TerminalEmulator = TerminalEmulator::Automatic;
pub(crate) const MIN_UNBOUNDED_COLUMN_WIDTH: f32 = 180.0;
pub(crate) const MAX_UNBOUNDED_COLUMN_WIDTH: f32 = 520.0;
pub(crate) const COLUMN_FIXED_COUNT_OPTIONS: [usize; 4] = [2, 3, 4, 5];

#[derive(Debug, Clone)]
pub(crate) struct UserConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) show_hidden_files: bool,
    pub(crate) column_view_mode: ColumnViewMode,
    pub(crate) column_fixed_count: usize,
    pub(crate) unbounded_column_width: f32,
    pub(crate) terminal_emulator: TerminalEmulator,
}

pub(crate) fn load_user_config() -> UserConfig {
    let default = default_user_config();
    let Some(config_file) = config_file_path() else {
        return default;
    };

    match fs::read_to_string(&config_file) {
        Ok(content) => parse_user_config(&content, default),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let _ = write_user_config(&config_file, &default);
            default
        }
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
        column_view_mode: DEFAULT_COLUMN_VIEW_MODE,
        column_fixed_count: DEFAULT_COLUMN_FIXED_COUNT,
        unbounded_column_width: DEFAULT_UNBOUNDED_COLUMN_WIDTH,
        terminal_emulator: DEFAULT_TERMINAL_EMULATOR,
    }
}

pub(crate) fn normalize_column_fixed_count(count: usize) -> usize {
    if COLUMN_FIXED_COUNT_OPTIONS.contains(&count) {
        count
    } else {
        DEFAULT_COLUMN_FIXED_COUNT
    }
}

pub(crate) fn normalize_unbounded_column_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_UNBOUNDED_COLUMN_WIDTH, MAX_UNBOUNDED_COLUMN_WIDTH)
    } else {
        DEFAULT_UNBOUNDED_COLUMN_WIDTH
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
            COLUMN_VIEW_MODE_KEY => {
                if let Some(mode) = parse_column_view_mode(value) {
                    config.column_view_mode = mode;
                }
            }
            COLUMN_FIXED_COUNT_KEY => {
                if let Ok(count) = value.parse::<usize>() {
                    config.column_fixed_count = normalize_column_fixed_count(count);
                }
            }
            UNBOUNDED_COLUMN_WIDTH_KEY => {
                if let Ok(width) = value.parse::<f32>() {
                    config.unbounded_column_width = normalize_unbounded_column_width(width);
                }
            }
            TERMINAL_EMULATOR_KEY => {
                if let Some(terminal_emulator) = TerminalEmulator::from_config_value(value) {
                    config.terminal_emulator = terminal_emulator;
                }
            }
            _ => {}
        }
    }
    config
}

fn parse_column_view_mode(value: &str) -> Option<ColumnViewMode> {
    match value {
        "unbounded" => Some(ColumnViewMode::Unbounded),
        "fixed" => Some(ColumnViewMode::Fixed),
        _ => None,
    }
}

fn column_view_mode_value(mode: ColumnViewMode) -> &'static str {
    match mode {
        ColumnViewMode::Unbounded => "unbounded",
        ColumnViewMode::Fixed => "fixed",
    }
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
            "# File Manager user configuration\n{}={}\n{}={}\n{}={}\n{}={}\n{}={}\n{}={}\n{}={}\n",
            SEARCH_INDEX_DIR_KEY,
            config.search_index_dir.to_string_lossy(),
            THUMBNAIL_CACHE_DIR_KEY,
            config.thumbnail_cache_dir.to_string_lossy(),
            SHOW_HIDDEN_FILES_KEY,
            config.show_hidden_files,
            COLUMN_VIEW_MODE_KEY,
            column_view_mode_value(config.column_view_mode),
            COLUMN_FIXED_COUNT_KEY,
            config.column_fixed_count,
            UNBOUNDED_COLUMN_WIDTH_KEY,
            config.unbounded_column_width,
            TERMINAL_EMULATOR_KEY,
            config.terminal_emulator.config_value()
        ),
    )
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
    fn parses_column_view_preferences() {
        let parsed = parse_user_config(
            "show_hidden_files=true\ncolumn_view_mode=fixed\ncolumn_fixed_count=5\nunbounded_column_width=320.5\nterminal_emulator=ghostty\n",
            default_user_config(),
        );

        assert!(parsed.show_hidden_files);
        assert_eq!(parsed.column_view_mode, ColumnViewMode::Fixed);
        assert_eq!(parsed.column_fixed_count, 5);
        assert_eq!(parsed.unbounded_column_width, 320.5);
        assert_eq!(parsed.terminal_emulator, TerminalEmulator::Ghostty);
    }

    #[test]
    fn invalid_column_view_preferences_fall_back_to_defaults() {
        let default = default_user_config();
        let parsed = parse_user_config(
            "show_hidden_files=maybe\ncolumn_view_mode=wide\ncolumn_fixed_count=8\nunbounded_column_width=nan\nterminal_emulator=missing\n",
            default.clone(),
        );

        assert_eq!(parsed.show_hidden_files, default.show_hidden_files);
        assert_eq!(parsed.column_view_mode, default.column_view_mode);
        assert_eq!(parsed.column_fixed_count, DEFAULT_COLUMN_FIXED_COUNT);
        assert_eq!(
            parsed.unbounded_column_width,
            DEFAULT_UNBOUNDED_COLUMN_WIDTH
        );
        assert_eq!(parsed.terminal_emulator, DEFAULT_TERMINAL_EMULATOR);
    }
}
