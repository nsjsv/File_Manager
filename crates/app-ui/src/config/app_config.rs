use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{
    app_config_dir_path, default_user_config, toml_string, RenderingGpuPreference, UserConfig,
    CONFIG_FILE_NAME,
};

const SEARCH_INDEX_DIR_KEY: &str = "search_index_dir";
const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const RENDERING_BACKEND_KEY: &str = "rendering_backend";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppConfig {
    pub(crate) search_index_dir: PathBuf,
    pub(crate) thumbnail_cache_dir: PathBuf,
    pub(crate) rendering_gpu_preference: RenderingGpuPreference,
}

impl AppConfig {
    pub(crate) fn from_user_config(config: &UserConfig) -> Self {
        Self {
            search_index_dir: config.search_index_dir.clone(),
            thumbnail_cache_dir: config.thumbnail_cache_dir.clone(),
            rendering_gpu_preference: config.rendering_gpu_preference,
        }
    }

    pub(crate) fn apply_to_user_config(&self, config: &mut UserConfig) {
        config.search_index_dir = self.search_index_dir.clone();
        config.thumbnail_cache_dir = self.thumbnail_cache_dir.clone();
        config.rendering_gpu_preference = self.rendering_gpu_preference;
    }
}

pub(crate) fn default_app_config() -> AppConfig {
    AppConfig::from_user_config(&default_user_config())
}

pub(crate) fn load_app_config() -> AppConfig {
    let default = default_app_config();
    let Some(config_dir) = app_config_dir_path() else {
        return default;
    };

    load_app_config_from_dir(&config_dir, default)
}

pub(crate) fn save_app_config(config: &AppConfig) -> io::Result<()> {
    let config_file = config_file_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "application configuration directory is unavailable",
        )
    })?;
    write_app_config(&config_file, config)
}

pub(super) fn load_app_config_from_dir(config_dir: &Path, default: AppConfig) -> AppConfig {
    let config_file = config_dir.join(CONFIG_FILE_NAME);
    match fs::read_to_string(&config_file) {
        Ok(content) => parse_toml_app_config(&content, default),
        Err(error) if error.kind() == io::ErrorKind::NotFound => default,
        Err(_) => default,
    }
}

pub(super) fn parse_toml_app_config(content: &str, default: AppConfig) -> AppConfig {
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
    if let Some(value) = toml_string(&document, RENDERING_BACKEND_KEY) {
        if let Some(preference) = RenderingGpuPreference::from_config_value(value) {
            config.rendering_gpu_preference = preference;
        }
    }
    config
}

pub(super) fn write_app_config(path: &Path, config: &AppConfig) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = config.search_index_dir.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(parent) = config.thumbnail_cache_dir.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let content = toml_app_config_content(config).map_err(io::Error::other)?;
    fs::write(path, content)
}

pub(super) fn toml_app_config_content(config: &AppConfig) -> Result<String, toml::ser::Error> {
    let mut document = toml::Table::new();
    document.insert(
        SEARCH_INDEX_DIR_KEY.to_owned(),
        toml::Value::String(config.search_index_dir.to_string_lossy().into_owned()),
    );
    document.insert(
        THUMBNAIL_CACHE_DIR_KEY.to_owned(),
        toml::Value::String(config.thumbnail_cache_dir.to_string_lossy().into_owned()),
    );
    document.insert(
        RENDERING_BACKEND_KEY.to_owned(),
        toml::Value::String(config.rendering_gpu_preference.config_value().to_owned()),
    );

    let content = toml::to_string_pretty(&document)?;
    Ok(format!(
        "# File Manager application configuration\n{content}"
    ))
}

fn config_file_path() -> Option<PathBuf> {
    app_config_dir_path().map(|path| path.join(CONFIG_FILE_NAME))
}
