use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{StoreError, StoreResult, StoredPath, TaskQueueStore};

pub const USER_PREFERENCES_KEY: &str = "main";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredUserPreferences {
    pub search_index_exclude_patterns: Vec<String>,
    pub search_index_media_scope: String,
    pub search_index_directory_error_policy: String,
    pub search_mode: String,
    pub search_mode_prompt: String,
    pub network_list_thumbnail_downloads_enabled: bool,
    pub max_preview_file_bytes: u64,
    pub show_hidden_files: bool,
    #[serde(default = "default_language_setting")]
    pub language_setting: String,
    pub sidebar_width: f64,
    pub sidebar_favorites: Option<Vec<StoredSidebarFavorite>>,
    pub network_connections: Vec<StoredNetworkConnection>,
    pub terminal_emulator: String,
    pub file_operation_verification: String,
    pub browser_view_mode: String,
    pub startup_location: String,
    pub startup_custom_directory: StoredPath,
    pub save_view_state: bool,
    pub shortcuts: Vec<StoredShortcutBinding>,
    #[serde(default = "default_stored_list_view_columns")]
    pub list_view_columns: Vec<StoredListViewColumn>,
    #[serde(default = "default_list_sort_field")]
    pub list_sort_field: String,
    #[serde(default = "default_list_sort_direction")]
    pub list_sort_direction: String,
    #[serde(default = "default_list_directory_size_display_mode")]
    pub list_directory_size_display_mode: String,
}

impl Default for StoredUserPreferences {
    fn default() -> Self {
        Self {
            search_index_exclude_patterns: Vec::new(),
            search_index_media_scope: "off".to_owned(),
            search_index_directory_error_policy: "skip_unreadable".to_owned(),
            search_mode: "simple".to_owned(),
            search_mode_prompt: "pending".to_owned(),
            network_list_thumbnail_downloads_enabled: false,
            max_preview_file_bytes: 3 * 1024 * 1024,
            show_hidden_files: false,
            language_setting: default_language_setting(),
            sidebar_width: 180.0,
            sidebar_favorites: None,
            network_connections: Vec::new(),
            terminal_emulator: "automatic".to_owned(),
            file_operation_verification: "basic_metadata".to_owned(),
            browser_view_mode: "columns".to_owned(),
            startup_location: "home".to_owned(),
            startup_custom_directory: StoredPath::from_path(Path::new("")),
            save_view_state: false,
            shortcuts: Vec::new(),
            list_view_columns: default_stored_list_view_columns(),
            list_sort_field: default_list_sort_field(),
            list_sort_direction: default_list_sort_direction(),
            list_directory_size_display_mode: default_list_directory_size_display_mode(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredListViewColumn {
    pub kind: String,
    pub width: f64,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSidebarFavorite {
    pub label: String,
    pub path: StoredPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredNetworkConnection {
    pub id: String,
    pub label: String,
    pub protocol: String,
    pub uri: String,
    pub auto_connect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredShortcutBinding {
    pub action_key: String,
    pub binding: String,
}

fn default_stored_list_view_columns() -> Vec<StoredListViewColumn> {
    [
        ("name", 320.0, true),
        ("modified", 168.0, true),
        ("size", 96.0, true),
        ("kind", 96.0, true),
        ("owner", 120.0, false),
        ("group", 120.0, false),
        ("permissions", 128.0, false),
        ("accessed", 168.0, false),
        ("created", 168.0, false),
    ]
    .into_iter()
    .map(|(kind, width, visible)| StoredListViewColumn {
        kind: kind.to_owned(),
        width,
        visible,
    })
    .collect()
}

fn default_list_sort_field() -> String {
    "name".to_owned()
}

fn default_list_sort_direction() -> String {
    "ascending".to_owned()
}

fn default_list_directory_size_display_mode() -> String {
    "item_count".to_owned()
}

fn default_language_setting() -> String {
    "system".to_owned()
}

impl TaskQueueStore {
    pub fn read_user_preferences(&self) -> StoreResult<Option<StoredUserPreferences>> {
        let connection = self.connection()?;
        let payload_json = connection
            .query_row(
                "SELECT payload_json FROM user_preferences WHERE preference_key = ?1",
                rusqlite::params![USER_PREFERENCES_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match payload_json {
            Some(payload_json) => match serde_json::from_str(&payload_json) {
                Ok(preferences) => Ok(Some(preferences)),
                Err(error) => {
                    connection.execute(
                        "DELETE FROM user_preferences WHERE preference_key = ?1",
                        rusqlite::params![USER_PREFERENCES_KEY],
                    )?;
                    Err(StoreError::Json(error))
                }
            },
            None => Ok(None),
        }
    }

    pub fn replace_user_preferences(&self, preferences: &StoredUserPreferences) -> StoreResult<()> {
        let connection = self.connection()?;
        let payload_json = serde_json::to_string(preferences)?;
        connection.execute(
            "INSERT INTO user_preferences (preference_key, payload_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(preference_key) DO UPDATE SET
                 payload_json = excluded.payload_json,
                 updated_at_ms = excluded.updated_at_ms",
            rusqlite::params![USER_PREFERENCES_KEY, payload_json, crate::current_time_ms()],
        )?;
        Ok(())
    }
}
