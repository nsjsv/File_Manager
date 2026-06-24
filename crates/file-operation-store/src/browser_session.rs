use serde::{Deserialize, Serialize};

use crate::StoredPath;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredBrowserViewMode {
    Columns,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredSplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoredBrowserPaneLayout {
    Single {
        active: u64,
    },
    Split {
        axis: StoredSplitAxis,
        first: u64,
        second: u64,
        active: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredColumnViewport {
    pub directory: StoredPath,
    pub offset_y: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredBrowserTab {
    pub id: u64,
    pub directory: StoredPath,
    pub is_trash_view: bool,
    pub selected: Option<StoredPath>,
    pub selected_paths: Vec<StoredPath>,
    pub deepest_open_column_directory: Option<StoredPath>,
    pub expanded_directories: Vec<StoredPath>,
    pub view_mode: StoredBrowserViewMode,
    pub back_stack: Vec<StoredPath>,
    pub forward_stack: Vec<StoredPath>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredBrowserPane {
    pub id: u64,
    pub tabs: Vec<StoredBrowserTab>,
    pub active_tab_id: u64,
    pub column_viewports: Vec<StoredColumnViewport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredSearchScope {
    CurrentDirectory,
    HomeDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredSearchMode {
    Files,
    Contents,
    Media,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSearchSession {
    pub scope: StoredSearchScope,
    pub mode: StoredSearchMode,
    pub root: StoredPath,
    pub query: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredFilePropertiesCategory {
    Information,
    Permissions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredPropertiesSession {
    pub path: StoredPath,
    pub category: StoredFilePropertiesCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredSettingsCategory {
    General,
    Network,
    SearchIndex,
    FileOperations,
    Rendering,
    Shortcuts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredBrowserSession {
    pub panes: Vec<StoredBrowserPane>,
    pub layout: StoredBrowserPaneLayout,
    pub search: Option<StoredSearchSession>,
    pub preview_path: Option<StoredPath>,
    pub properties: Option<StoredPropertiesSession>,
    pub settings_category: Option<StoredSettingsCategory>,
}
