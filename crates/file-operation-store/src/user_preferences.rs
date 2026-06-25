use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::{StoreError, StoreResult, StoredPath, TaskQueueStore};

pub const USER_PREFERENCES_KEY: &str = "main";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredUserPreferences {
    pub search_index_exclude_patterns: Vec<String>,
    pub search_index_content_enabled: bool,
    pub search_index_media_scope: String,
    pub search_index_directory_error_policy: String,
    pub search_mode: String,
    pub search_mode_prompt: String,
    pub network_list_thumbnail_downloads_enabled: bool,
    pub max_preview_file_bytes: u64,
    pub show_hidden_files: bool,
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
