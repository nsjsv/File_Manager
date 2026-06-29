use std::path::{Path, PathBuf};

use desktop_linux::{NetworkConnection, NetworkConnectionId, NetworkProtocol, TerminalEmulator};
use file_core::FileOperationVerification;
use file_index::{DirectoryErrorPolicy, MediaMetadataScope};
use file_operation_store::{
    StoreResult, StoredNetworkConnection, StoredPath, StoredShortcutBinding, StoredSidebarFavorite,
    StoredUserPreferences, TaskQueueStore,
};

use super::app_config::AppConfig;
use super::legacy_toml;
use super::startup::StartupLocationPolicy;
use super::{
    app_config_dir_path, browser_view_mode_config_value, browser_view_mode_from_config_value,
    default_state_database_path, default_user_config, file_operation_verification_config_value,
    file_operation_verification_from_config_value, normalize_max_preview_file_bytes,
    normalize_search_index_exclude_patterns, normalize_sidebar_width, SearchBackendMode,
    SearchModePromptStatus, SidebarFavoriteConfig, UserConfig,
};
use crate::model::BrowserViewMode;
use crate::network_connections::SavedNetworkConnection;
use crate::shortcuts::ShortcutConfig;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserPreferences {
    pub(crate) search_index_exclude_patterns: Vec<String>,
    pub(crate) search_index_content_enabled: bool,
    pub(crate) search_index_media_scope: MediaMetadataScope,
    pub(crate) search_index_directory_error_policy: DirectoryErrorPolicy,
    pub(crate) search_mode: SearchBackendMode,
    pub(crate) search_mode_prompt: SearchModePromptStatus,
    pub(crate) network_list_thumbnail_downloads_enabled: bool,
    pub(crate) max_preview_file_bytes: u64,
    pub(crate) show_hidden_files: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) network_connections: Vec<SavedNetworkConnection>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) file_operation_verification: FileOperationVerification,
    pub(crate) browser_view_mode: BrowserViewMode,
    pub(crate) startup_location_policy: StartupLocationPolicy,
    pub(crate) startup_custom_directory: PathBuf,
    pub(crate) save_view_state: bool,
    pub(crate) shortcuts: ShortcutConfig,
}

impl UserPreferences {
    pub(crate) fn from_user_config(config: &UserConfig) -> Self {
        Self {
            search_index_exclude_patterns: normalize_search_index_exclude_patterns(
                config.search_index_exclude_patterns.clone(),
            ),
            search_index_content_enabled: config.search_index_content_enabled,
            search_index_media_scope: config.search_index_media_scope,
            search_index_directory_error_policy: config.search_index_directory_error_policy,
            search_mode: config.search_mode,
            search_mode_prompt: config.search_mode_prompt,
            network_list_thumbnail_downloads_enabled: config
                .network_list_thumbnail_downloads_enabled,
            max_preview_file_bytes: normalize_max_preview_file_bytes(config.max_preview_file_bytes),
            show_hidden_files: config.show_hidden_files,
            sidebar_width: normalize_sidebar_width(config.sidebar_width),
            sidebar_favorites: config.sidebar_favorites.clone(),
            network_connections: config.network_connections.clone(),
            terminal_emulator: config.terminal_emulator,
            file_operation_verification: config.file_operation_verification,
            browser_view_mode: config.browser_view_mode,
            startup_location_policy: config.startup_location_policy,
            startup_custom_directory: config.startup_custom_directory.clone(),
            save_view_state: config.startup_location_policy.saves_view_state(),
            shortcuts: config.shortcuts.clone(),
        }
    }

    pub(crate) fn apply_to_user_config(&self, config: &mut UserConfig) {
        config.search_index_exclude_patterns = self.search_index_exclude_patterns.clone();
        config.search_index_content_enabled = self.search_index_content_enabled;
        config.search_index_media_scope = self.search_index_media_scope;
        config.search_index_directory_error_policy = self.search_index_directory_error_policy;
        config.search_mode = self.search_mode;
        config.search_mode_prompt = self.search_mode_prompt;
        config.network_list_thumbnail_downloads_enabled =
            self.network_list_thumbnail_downloads_enabled;
        config.max_preview_file_bytes = self.max_preview_file_bytes;
        config.show_hidden_files = self.show_hidden_files;
        config.sidebar_width = self.sidebar_width;
        config.sidebar_favorites = self.sidebar_favorites.clone();
        config.network_connections = self.network_connections.clone();
        config.terminal_emulator = self.terminal_emulator;
        config.file_operation_verification = self.file_operation_verification;
        config.browser_view_mode = self.browser_view_mode;
        config.startup_location_policy = self.startup_location_policy;
        config.startup_custom_directory = self.startup_custom_directory.clone();
        config.save_view_state = self.startup_location_policy.saves_view_state();
        config.shortcuts = self.shortcuts.clone();
    }

    pub(crate) fn to_stored(&self) -> StoredUserPreferences {
        StoredUserPreferences {
            search_index_exclude_patterns: self.search_index_exclude_patterns.clone(),
            search_index_content_enabled: self.search_index_content_enabled,
            search_index_media_scope: self.search_index_media_scope.config_value().to_owned(),
            search_index_directory_error_policy: self
                .search_index_directory_error_policy
                .config_value()
                .to_owned(),
            search_mode: self.search_mode.config_value().to_owned(),
            search_mode_prompt: self.search_mode_prompt.config_value().to_owned(),
            network_list_thumbnail_downloads_enabled: self.network_list_thumbnail_downloads_enabled,
            max_preview_file_bytes: normalize_max_preview_file_bytes(self.max_preview_file_bytes),
            show_hidden_files: self.show_hidden_files,
            sidebar_width: f64::from(normalize_sidebar_width(self.sidebar_width)),
            sidebar_favorites: self
                .sidebar_favorites
                .as_ref()
                .map(|favorites| stored_sidebar_favorites(favorites)),
            network_connections: stored_network_connections(&self.network_connections),
            terminal_emulator: self.terminal_emulator.config_value().to_owned(),
            file_operation_verification: file_operation_verification_config_value(
                self.file_operation_verification,
            )
            .to_owned(),
            browser_view_mode: browser_view_mode_config_value(self.browser_view_mode).to_owned(),
            startup_location: self.startup_location_policy.config_value().to_owned(),
            startup_custom_directory: StoredPath::from_path(&self.startup_custom_directory),
            save_view_state: self.startup_location_policy.saves_view_state(),
            shortcuts: stored_shortcuts(&self.shortcuts),
        }
    }

    pub(crate) fn from_stored(stored: StoredUserPreferences, default: &UserConfig) -> Self {
        let default_preferences = Self::from_user_config(default);
        let startup_location_policy =
            StartupLocationPolicy::from_config_value(&stored.startup_location)
                .unwrap_or(default_preferences.startup_location_policy);
        Self {
            search_index_exclude_patterns: normalize_search_index_exclude_patterns(
                stored.search_index_exclude_patterns,
            ),
            search_index_content_enabled: stored.search_index_content_enabled,
            search_index_media_scope: MediaMetadataScope::from_config_value(
                &stored.search_index_media_scope,
            )
            .unwrap_or(default_preferences.search_index_media_scope),
            search_index_directory_error_policy: DirectoryErrorPolicy::from_config_value(
                &stored.search_index_directory_error_policy,
            )
            .unwrap_or(default_preferences.search_index_directory_error_policy),
            search_mode: SearchBackendMode::from_config_value(&stored.search_mode)
                .unwrap_or(default_preferences.search_mode),
            search_mode_prompt: SearchModePromptStatus::from_config_value(
                &stored.search_mode_prompt,
            )
            .unwrap_or(default_preferences.search_mode_prompt),
            network_list_thumbnail_downloads_enabled: stored
                .network_list_thumbnail_downloads_enabled,
            max_preview_file_bytes: normalize_max_preview_file_bytes(stored.max_preview_file_bytes),
            show_hidden_files: stored.show_hidden_files,
            sidebar_width: normalize_sidebar_width(stored.sidebar_width as f32),
            sidebar_favorites: stored
                .sidebar_favorites
                .map(|favorites| sidebar_favorites_from_stored(&favorites)),
            network_connections: network_connections_from_stored(&stored.network_connections),
            terminal_emulator: TerminalEmulator::from_config_value(&stored.terminal_emulator)
                .unwrap_or(default_preferences.terminal_emulator),
            file_operation_verification: file_operation_verification_from_config_value(
                &stored.file_operation_verification,
            )
            .unwrap_or(default_preferences.file_operation_verification),
            browser_view_mode: browser_view_mode_from_config_value(&stored.browser_view_mode)
                .unwrap_or(default_preferences.browser_view_mode),
            startup_location_policy,
            startup_custom_directory: stored.startup_custom_directory.to_path_buf(),
            save_view_state: startup_location_policy.saves_view_state(),
            shortcuts: shortcut_config_from_stored(&stored.shortcuts),
        }
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self::from_user_config(&default_user_config())
    }
}

impl UserConfig {
    pub(crate) fn from_parts(app_config: AppConfig, preferences: UserPreferences) -> Self {
        let mut config = default_user_config();
        app_config.apply_to_user_config(&mut config);
        preferences.apply_to_user_config(&mut config);
        config
    }

    pub(crate) fn user_preferences(&self) -> UserPreferences {
        UserPreferences::from_user_config(self)
    }
}

pub(crate) fn load_user_config_for_app_config(app_config: AppConfig) -> UserConfig {
    let state_database_path = default_state_database_path();
    let config_dir = app_config_dir_path();
    load_user_config_from_sources(app_config, &state_database_path, config_dir.as_deref())
}

pub(super) fn load_user_config_from_sources(
    app_config: AppConfig,
    state_database_path: &Path,
    config_dir: Option<&Path>,
) -> UserConfig {
    let mut default = default_user_config();
    app_config.apply_to_user_config(&mut default);

    let Ok(store) = TaskQueueStore::new(state_database_path) else {
        return default;
    };

    match store.read_user_preferences() {
        Ok(Some(stored)) => {
            let preferences = UserPreferences::from_stored(stored, &default);
            UserConfig::from_parts(app_config, preferences)
        }
        Ok(None) => {
            let migrated = config_dir.map_or_else(
                || default.clone(),
                |config_dir| {
                    legacy_toml::load_legacy_user_config_from_dir(config_dir, default.clone())
                },
            );
            let _ = store.replace_user_preferences(&migrated.user_preferences().to_stored());
            migrated
        }
        Err(_) => default,
    }
}

pub(crate) fn save_user_preferences(
    store: &TaskQueueStore,
    preferences: &UserPreferences,
) -> StoreResult<()> {
    store.replace_user_preferences(&preferences.to_stored())
}

fn stored_sidebar_favorites(favorites: &[SidebarFavoriteConfig]) -> Vec<StoredSidebarFavorite> {
    favorites
        .iter()
        .map(|favorite| StoredSidebarFavorite {
            label: favorite.label.clone(),
            path: StoredPath::from_path(&favorite.path),
        })
        .collect()
}

fn sidebar_favorites_from_stored(
    favorites: &[StoredSidebarFavorite],
) -> Vec<SidebarFavoriteConfig> {
    favorites
        .iter()
        .map(|favorite| SidebarFavoriteConfig {
            label: favorite.label.clone(),
            path: favorite.path.to_path_buf(),
        })
        .collect()
}

fn stored_network_connections(
    connections: &[SavedNetworkConnection],
) -> Vec<StoredNetworkConnection> {
    connections
        .iter()
        .map(|saved| StoredNetworkConnection {
            id: saved.connection.id.as_str().to_owned(),
            label: saved.connection.label.clone(),
            protocol: saved.connection.protocol.config_value().to_owned(),
            uri: saved.connection.uri.clone(),
            auto_connect: saved.auto_connect,
        })
        .collect()
}

fn network_connections_from_stored(
    connections: &[StoredNetworkConnection],
) -> Vec<SavedNetworkConnection> {
    let mut restored = Vec::new();
    for stored in connections {
        let Some(protocol) = NetworkProtocol::from_config_value(&stored.protocol) else {
            continue;
        };
        if let Ok(connection) = NetworkConnection::new(
            NetworkConnectionId::new(stored.id.trim()),
            stored.label.trim().to_owned(),
            protocol,
            &stored.uri,
        ) {
            restored.push(SavedNetworkConnection::new(connection, stored.auto_connect));
        }
    }
    restored
}

fn stored_shortcuts(shortcuts: &ShortcutConfig) -> Vec<StoredShortcutBinding> {
    shortcuts
        .toml_table()
        .into_iter()
        .filter_map(|(action_key, value)| {
            let binding = value.as_str()?.to_owned();
            Some(StoredShortcutBinding {
                action_key,
                binding,
            })
        })
        .collect()
}

fn shortcut_config_from_stored(shortcuts: &[StoredShortcutBinding]) -> ShortcutConfig {
    let mut config = ShortcutConfig::defaults();
    let table = shortcuts
        .iter()
        .map(|binding| {
            (
                binding.action_key.clone(),
                toml::Value::String(binding.binding.clone()),
            )
        })
        .collect();
    config.apply_toml_table(&table);
    config
}
