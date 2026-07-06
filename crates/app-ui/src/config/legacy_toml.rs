use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use desktop_linux::{NetworkConnection, NetworkConnectionId, NetworkProtocol, TerminalEmulator};

use super::startup;
use super::{
    browser_view_mode_from_config_value, file_operation_verification_from_config_value,
    normalize_max_preview_file_bytes, normalize_sidebar_width, toml_string, SidebarFavoriteConfig,
    UserConfig, CONFIG_FILE_NAME,
};
use crate::network_connections::SavedNetworkConnection;

const THUMBNAIL_CACHE_DIR_KEY: &str = "thumbnail_cache_dir";
const NETWORK_LIST_THUMBNAIL_DOWNLOADS_ENABLED_KEY: &str =
    "network_list_thumbnail_downloads_enabled";
const MAX_PREVIEW_FILE_BYTES_KEY: &str = "max_preview_file_bytes";
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
const NETWORK_CONNECTION_AUTO_CONNECT_KEY: &str = "auto_connect";
const TERMINAL_EMULATOR_KEY: &str = "terminal_emulator";
const RENDERING_BACKEND_KEY: &str = "rendering_backend";
const FILE_OPERATION_VERIFICATION_KEY: &str = "file_operation_verification";
const BROWSER_VIEW_MODE_KEY: &str = "browser_view_mode";
const SHORTCUTS_KEY: &str = "shortcuts";

pub(super) fn load_legacy_user_config_from_dir(
    config_dir: &Path,
    default: UserConfig,
) -> UserConfig {
    let config_file = config_dir.join(CONFIG_FILE_NAME);
    match fs::read_to_string(&config_file) {
        Ok(content) => parse_toml_user_config(&content, default),
        Err(error) if error.kind() == io::ErrorKind::NotFound => default,
        Err(_) => default,
    }
}

pub(super) fn parse_toml_user_config(content: &str, default: UserConfig) -> UserConfig {
    let Ok(document) = content.parse::<toml::Table>() else {
        return default;
    };

    let mut config = default;
    if let Some(value) = toml_string(&document, THUMBNAIL_CACHE_DIR_KEY) {
        config.thumbnail_cache_dir = PathBuf::from(value);
    }
    if let Some(value) = document
        .get(NETWORK_LIST_THUMBNAIL_DOWNLOADS_ENABLED_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.network_list_thumbnail_downloads_enabled = value;
    }
    if let Some(bytes) = document
        .get(MAX_PREVIEW_FILE_BYTES_KEY)
        .and_then(toml_positive_integer_as_u64)
    {
        config.max_preview_file_bytes = normalize_max_preview_file_bytes(bytes);
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
        if let Some(preference) = super::RenderingGpuPreference::from_config_value(value) {
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
    startup::apply_toml_startup_config(&mut config, &document);
    if let Some(table) = document.get(SHORTCUTS_KEY).and_then(toml::Value::as_table) {
        config.shortcuts.apply_toml_table(table);
    }
    config
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

fn parse_toml_network_connections(document: &toml::Table) -> Vec<SavedNetworkConnection> {
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
        let auto_connect = table
            .get(NETWORK_CONNECTION_AUTO_CONNECT_KEY)
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if let Ok(connection) =
            NetworkConnection::new(NetworkConnectionId::new(id.trim()), label, protocol, uri)
        {
            connections.push(SavedNetworkConnection::new(connection, auto_connect));
        }
    }
    connections
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

fn toml_positive_integer_as_u64(value: &toml::Value) -> Option<u64> {
    match value {
        toml::Value::Integer(value) => (*value > 0).then_some(*value as u64),
        _ => None,
    }
}
