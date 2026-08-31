use std::path::{Path, PathBuf};

use desktop_linux::{NetworkConnection, NetworkConnectionId, NetworkProtocol, TerminalEmulator};
use file_core::FileOperationVerification;
use file_operation_store::{
    StoreResult, StoredListViewColumn, StoredNetworkConnection, StoredPath, StoredShortcutBinding,
    StoredSidebarFavorite, StoredUserPreferences, StoredWindowControlPlacement, TaskQueueStore,
};

use super::app_config::AppConfig;
use super::legacy_toml;
use super::startup::StartupLocationPolicy;
use super::{
    app_config_dir_path, browser_view_mode_config_value, browser_view_mode_from_config_value,
    default_state_database_path, default_user_config, file_operation_verification_config_value,
    file_operation_verification_from_config_value, list_directory_size_display_mode_config_value,
    list_directory_size_display_mode_from_config_value, normalize_icon_grid_size,
    normalize_preview_directory_expand_levels, normalize_sidebar_width,
    sort_direction_config_value, sort_direction_from_config_value, sort_field_config_value,
    sort_field_from_config_value, LaunchWindowPolicy, PreviewFileSizeLimits, SidebarFavoriteConfig,
    UiLanguageSetting, UserConfig,
};
use crate::matugen_theme::{ColorSchemePreset, CustomColorScheme, ThemeMode};
use crate::model::{
    list_column_kind_config_value, list_column_kind_from_config_value, BrowserViewMode,
    ListColumnConfig, ListDirectorySizeDisplayMode, ListSortPreference, ListViewPreferences,
    WindowChromeLayout, WindowControlKind, WindowControlPlacement, WindowControlSide,
    WindowControlVisibility, WindowControlsConfig,
};
use crate::network_connections::SavedNetworkConnection;
use crate::shortcuts::ShortcutConfig;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UserPreferences {
    pub(crate) network_list_thumbnail_downloads_enabled: bool,
    pub(crate) preview_size_limits: PreviewFileSizeLimits,
    pub(crate) preview_directory_expand_levels: u8,
    pub(crate) show_hidden_files: bool,
    pub(crate) language_setting: UiLanguageSetting,
    pub(crate) sidebar_width: f32,
    pub(crate) sidebar_favorites: Option<Vec<SidebarFavoriteConfig>>,
    pub(crate) network_connections: Vec<SavedNetworkConnection>,
    pub(crate) terminal_emulator: TerminalEmulator,
    pub(crate) file_operation_verification: FileOperationVerification,
    pub(crate) browser_view_mode: BrowserViewMode,
    pub(crate) window_controls: WindowControlsConfig,
    pub(crate) icon_grid_size: u32,
    pub(crate) list_view_preferences: ListViewPreferences,
    pub(crate) list_directory_size_display_mode: ListDirectorySizeDisplayMode,
    pub(crate) startup_location_policy: StartupLocationPolicy,
    pub(crate) launch_window_policy: LaunchWindowPolicy,
    pub(crate) startup_custom_directory: PathBuf,
    pub(crate) save_view_state: bool,
    pub(crate) shortcuts: ShortcutConfig,
    pub(crate) search_history: crate::model::SearchHistory,
    pub(crate) theme_mode: ThemeMode,
    pub(crate) color_scheme: ColorSchemePreset,
    pub(crate) custom_color_scheme: CustomColorScheme,
}

impl UserPreferences {
    pub(crate) fn from_user_config(config: &UserConfig) -> Self {
        Self {
            network_list_thumbnail_downloads_enabled: config
                .network_list_thumbnail_downloads_enabled,
            preview_size_limits: config.preview_size_limits,
            preview_directory_expand_levels: config.preview_directory_expand_levels,
            show_hidden_files: config.show_hidden_files,
            language_setting: config.language_setting,
            sidebar_width: normalize_sidebar_width(config.sidebar_width),
            sidebar_favorites: config.sidebar_favorites.clone(),
            network_connections: config.network_connections.clone(),
            terminal_emulator: config.terminal_emulator,
            file_operation_verification: config.file_operation_verification,
            browser_view_mode: config.browser_view_mode,
            window_controls: config.window_controls.clone(),
            icon_grid_size: normalize_icon_grid_size(config.icon_grid_size),
            list_view_preferences: config.list_view_preferences.clone(),
            list_directory_size_display_mode: config.list_directory_size_display_mode,
            startup_location_policy: config.startup_location_policy,
            launch_window_policy: config.launch_window_policy,
            startup_custom_directory: config.startup_custom_directory.clone(),
            save_view_state: config.startup_location_policy.saves_view_state(),
            shortcuts: config.shortcuts.clone(),
            search_history: config.search_history.clone(),
            theme_mode: config.theme_mode,
            color_scheme: config.color_scheme,
            custom_color_scheme: config.custom_color_scheme.clone(),
        }
    }

    pub(crate) fn apply_to_user_config(&self, config: &mut UserConfig) {
        config.network_list_thumbnail_downloads_enabled =
            self.network_list_thumbnail_downloads_enabled;
        config.preview_size_limits = self.preview_size_limits;
        config.preview_directory_expand_levels = self.preview_directory_expand_levels;
        config.show_hidden_files = self.show_hidden_files;
        config.language_setting = self.language_setting;
        config.sidebar_width = self.sidebar_width;
        config.sidebar_favorites = self.sidebar_favorites.clone();
        config.network_connections = self.network_connections.clone();
        config.terminal_emulator = self.terminal_emulator;
        config.file_operation_verification = self.file_operation_verification;
        config.browser_view_mode = self.browser_view_mode;
        config.window_controls = self.window_controls.clone();
        config.icon_grid_size = normalize_icon_grid_size(self.icon_grid_size);
        config.list_view_preferences = self.list_view_preferences.clone();
        config.list_directory_size_display_mode = self.list_directory_size_display_mode;
        config.startup_location_policy = self.startup_location_policy;
        config.launch_window_policy = self.launch_window_policy;
        config.startup_custom_directory = self.startup_custom_directory.clone();
        config.save_view_state = self.startup_location_policy.saves_view_state();
        config.shortcuts = self.shortcuts.clone();
        config.search_history = self.search_history.clone();
        config.theme_mode = self.theme_mode;
        config.color_scheme = self.color_scheme;
        config.custom_color_scheme = self.custom_color_scheme.clone();
    }

    pub(crate) fn to_stored(&self) -> StoredUserPreferences {
        let mut stored = StoredUserPreferences::default();
        stored.network_list_thumbnail_downloads_enabled =
            self.network_list_thumbnail_downloads_enabled;
        stored.max_preview_file_bytes = None;
        stored.preview_text_size_bytes = Some(self.preview_size_limits.text_bytes);
        stored.preview_image_size_bytes = Some(self.preview_size_limits.image_bytes);
        stored.preview_video_size_bytes = Some(self.preview_size_limits.video_bytes);
        stored.preview_audio_size_bytes = Some(self.preview_size_limits.audio_bytes);
        stored.preview_archive_size_bytes = Some(self.preview_size_limits.archive_bytes);
        stored.preview_document_size_bytes = Some(self.preview_size_limits.document_bytes);
        stored.preview_directory_expand_levels = Some(self.preview_directory_expand_levels);
        stored.show_hidden_files = self.show_hidden_files;
        stored.language_setting = self.language_setting.config_value().to_owned();
        stored.sidebar_width = f64::from(normalize_sidebar_width(self.sidebar_width));
        stored.sidebar_favorites = self
            .sidebar_favorites
            .as_ref()
            .map(|favorites| stored_sidebar_favorites(favorites));
        stored.network_connections = stored_network_connections(&self.network_connections);
        stored.terminal_emulator = self.terminal_emulator.config_value().to_owned();
        stored.file_operation_verification =
            file_operation_verification_config_value(self.file_operation_verification).to_owned();
        stored.browser_view_mode =
            browser_view_mode_config_value(self.browser_view_mode).to_owned();
        stored.window_chrome_layout = self.window_controls.layout().config_value().to_owned();
        stored.window_controls = stored_window_controls(&self.window_controls);
        stored.icon_grid_size = normalize_icon_grid_size(self.icon_grid_size);
        stored.list_view_columns = stored_list_view_columns(&self.list_view_preferences);
        stored.list_sort_field =
            sort_field_config_value(self.list_view_preferences.sort().field).to_owned();
        stored.list_sort_direction =
            sort_direction_config_value(self.list_view_preferences.sort().direction).to_owned();
        stored.list_directory_size_display_mode =
            list_directory_size_display_mode_config_value(self.list_directory_size_display_mode)
                .to_owned();
        stored.startup_location = self.startup_location_policy.config_value().to_owned();
        stored.launch_window_policy = self.launch_window_policy.config_value().to_owned();
        stored.startup_custom_directory = StoredPath::from_path(&self.startup_custom_directory);
        stored.save_view_state = self.startup_location_policy.saves_view_state();
        stored.shortcuts = stored_shortcuts(&self.shortcuts);
        stored.search_history = self.search_history.entries().to_vec();
        stored.theme_mode = self.theme_mode.config_value().to_owned();
        stored.color_scheme = self.color_scheme.config_value().to_owned();
        stored.custom_color_scheme = Some(self.custom_color_scheme.to_stored());
        stored
    }

    pub(crate) fn from_stored(stored: StoredUserPreferences, default: &UserConfig) -> Self {
        let default_preferences = Self::from_user_config(default);
        let (theme_mode, color_scheme) = match (
            ThemeMode::from_config_value(&stored.theme_mode),
            ColorSchemePreset::from_config_value(&stored.color_scheme),
        ) {
            (Some(theme_mode), Some(color_scheme)) => (theme_mode, color_scheme),
            _ => (
                default_preferences.theme_mode,
                default_preferences.color_scheme,
            ),
        };
        let startup_location_policy =
            StartupLocationPolicy::from_config_value(&stored.startup_location)
                .unwrap_or(default_preferences.startup_location_policy);
        let list_view_preferences =
            list_view_preferences_from_stored(&stored, &default_preferences);
        let custom_color_scheme = CustomColorScheme::from_stored(
            stored.custom_color_scheme.as_ref(),
            &default_preferences.custom_color_scheme,
        );
        let window_controls = window_controls_from_stored(&stored, &default_preferences);
        Self {
            network_list_thumbnail_downloads_enabled: stored
                .network_list_thumbnail_downloads_enabled,
            preview_size_limits: preview_size_limits_from_stored(&stored, &default_preferences),
            preview_directory_expand_levels: stored.preview_directory_expand_levels.map_or(
                default_preferences.preview_directory_expand_levels,
                normalize_preview_directory_expand_levels,
            ),
            show_hidden_files: stored.show_hidden_files,
            language_setting: UiLanguageSetting::from_config_value(&stored.language_setting)
                .unwrap_or(default_preferences.language_setting),
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
            window_controls,
            icon_grid_size: normalize_icon_grid_size(stored.icon_grid_size),
            list_view_preferences,
            list_directory_size_display_mode: list_directory_size_display_mode_from_config_value(
                &stored.list_directory_size_display_mode,
            )
            .unwrap_or(default_preferences.list_directory_size_display_mode),
            startup_location_policy,
            launch_window_policy: LaunchWindowPolicy::from_config_value(
                &stored.launch_window_policy,
            )
            .unwrap_or(default_preferences.launch_window_policy),
            startup_custom_directory: stored.startup_custom_directory.to_path_buf(),
            save_view_state: startup_location_policy.saves_view_state(),
            shortcuts: shortcut_config_from_stored(&stored.shortcuts),
            search_history: crate::model::SearchHistory::from_persisted(stored.search_history),
            theme_mode,
            color_scheme,
            custom_color_scheme,
        }
    }
}

fn preview_size_limits_from_stored(
    stored: &StoredUserPreferences,
    default: &UserPreferences,
) -> PreviewFileSizeLimits {
    let legacy_global_limit = stored.max_preview_file_bytes;
    let limit = |stored_bytes: Option<u64>, default_bytes: u64| {
        stored_bytes
            .or(legacy_global_limit)
            .unwrap_or(default_bytes)
    };
    PreviewFileSizeLimits {
        text_bytes: limit(
            stored.preview_text_size_bytes,
            default.preview_size_limits.text_bytes,
        ),
        image_bytes: limit(
            stored.preview_image_size_bytes,
            default.preview_size_limits.image_bytes,
        ),
        video_bytes: limit(
            stored.preview_video_size_bytes,
            default.preview_size_limits.video_bytes,
        ),
        audio_bytes: limit(
            stored.preview_audio_size_bytes,
            default.preview_size_limits.audio_bytes,
        ),
        archive_bytes: limit(
            stored.preview_archive_size_bytes,
            default.preview_size_limits.archive_bytes,
        ),
        document_bytes: limit(
            stored.preview_document_size_bytes,
            default.preview_size_limits.document_bytes,
        ),
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

fn stored_list_view_columns(preferences: &ListViewPreferences) -> Vec<StoredListViewColumn> {
    preferences
        .columns()
        .iter()
        .map(|column| StoredListViewColumn {
            kind: list_column_kind_config_value(column.kind).to_owned(),
            width: f64::from(column.width),
            visible: column.visible,
        })
        .collect()
}

fn list_view_preferences_from_stored(
    stored: &StoredUserPreferences,
    default: &UserPreferences,
) -> ListViewPreferences {
    let columns = stored
        .list_view_columns
        .iter()
        .filter_map(|column| {
            let kind = list_column_kind_from_config_value(&column.kind)?;
            Some(ListColumnConfig::new(
                kind,
                column.width as f32,
                column.visible,
            ))
        })
        .collect();
    let sort = ListSortPreference {
        field: sort_field_from_config_value(&stored.list_sort_field)
            .unwrap_or(default.list_view_preferences.sort().field),
        direction: sort_direction_from_config_value(&stored.list_sort_direction)
            .unwrap_or(default.list_view_preferences.sort().direction),
    };
    ListViewPreferences::new(columns, sort)
}

fn window_controls_from_stored(
    stored: &StoredUserPreferences,
    default: &UserPreferences,
) -> WindowControlsConfig {
    let layout = WindowChromeLayout::from_config_value(&stored.window_chrome_layout)
        .unwrap_or(default.window_controls.layout());
    let placements = stored
        .window_controls
        .iter()
        .filter_map(|placement| {
            let kind = WindowControlKind::from_config_value(&placement.kind)?;
            let side = WindowControlSide::from_config_value(&placement.side)
                .unwrap_or_else(|| default.window_controls.placement(kind).side());
            Some(WindowControlPlacement::new(
                kind,
                side,
                WindowControlVisibility::from(placement.visible),
            ))
        })
        .collect();
    WindowControlsConfig::from_partial_placements(layout, placements)
}

fn stored_window_controls(
    window_controls: &WindowControlsConfig,
) -> Vec<StoredWindowControlPlacement> {
    window_controls
        .placements()
        .iter()
        .map(|placement| StoredWindowControlPlacement {
            kind: placement.kind().config_value().to_owned(),
            side: placement.side().config_value().to_owned(),
            visible: placement.visibility().is_visible(),
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    const CUSTOM_JSON: &str = r##"{
        "version": 1,
        "light": {
            "background": "#ffffff",
            "surface": "#f6f8fa",
            "text": "#1f2328",
            "muted_text": "#59636e",
            "primary": "#0969da",
            "success": "#1a7f37",
            "warning": "#9a6700",
            "danger": "#d1242f"
        },
        "dark": {
            "background": "#0d1117",
            "surface": "#151b23",
            "text": "#f0f6fc",
            "muted_text": "#9198a1",
            "primary": "#4493f8",
            "success": "#3fb950",
            "warning": "#d29922",
            "danger": "#f85149"
        }
    }"##;

    #[test]
    fn custom_color_scheme_roundtrips_and_invalid_modes_fall_back_independently() {
        let default = default_user_config();
        let imported = CustomColorScheme::from_json(CUSTOM_JSON).expect("valid custom scheme");
        let mut config = default.clone();
        config.custom_color_scheme = imported.clone();
        config.color_scheme = ColorSchemePreset::Custom;

        let stored = config.user_preferences().to_stored();
        let restored = UserPreferences::from_stored(stored, &default);
        assert_eq!(restored.custom_color_scheme, imported);
        assert_eq!(restored.color_scheme, ColorSchemePreset::Custom);

        let mut partially_invalid = config.user_preferences().to_stored();
        let custom = partially_invalid
            .custom_color_scheme
            .as_mut()
            .expect("stored custom scheme");
        custom.dark.as_mut().expect("stored dark set").text = "bad".to_owned();
        let restored = UserPreferences::from_stored(partially_invalid, &default);
        assert_eq!(restored.custom_color_scheme.light, imported.light);
        assert_eq!(
            restored.custom_color_scheme.dark,
            default.custom_color_scheme.dark
        );
    }

    #[test]
    fn missing_custom_snapshot_uses_default_without_affecting_old_selection() {
        let mut stored = StoredUserPreferences::default();
        stored.color_scheme = "custom".to_owned();
        let restored = UserPreferences::from_stored(stored, &default_user_config());
        assert_eq!(restored.color_scheme, ColorSchemePreset::Custom);
        assert_eq!(
            restored.custom_color_scheme,
            default_user_config().custom_color_scheme
        );
    }

    #[test]
    fn theme_selection_roundtrips_through_stored_preferences() {
        for color_scheme in ColorSchemePreset::ALL {
            let mut config = default_user_config();
            config.theme_mode = ThemeMode::Dark;
            config.color_scheme = color_scheme;

            let stored = config.user_preferences().to_stored();
            let restored = UserPreferences::from_stored(stored, &default_user_config());

            assert_eq!(restored.theme_mode, ThemeMode::Dark);
            assert_eq!(restored.color_scheme, color_scheme);
        }
    }

    #[test]
    fn invalid_theme_selection_falls_back_to_current_defaults() {
        for (theme_mode, color_scheme) in [("sepia", "claude"), ("dark", "unknown")] {
            let mut stored = StoredUserPreferences::default();
            stored.theme_mode = theme_mode.to_owned();
            stored.color_scheme = color_scheme.to_owned();

            let restored = UserPreferences::from_stored(stored, &default_user_config());

            assert_eq!(restored.theme_mode, ThemeMode::Automatic);
            assert_eq!(restored.color_scheme, ColorSchemePreset::Default);
        }
    }
}
