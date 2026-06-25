use std::path::PathBuf;

use toml::Table;

use super::{toml_string, UserConfig};

const STARTUP_LOCATION_KEY: &str = "startup_location";
const STARTUP_CUSTOM_DIRECTORY_KEY: &str = "startup_custom_directory";
const SAVE_VIEW_STATE_KEY: &str = "save_view_state";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupLocationPolicy {
    Home,
    CustomDirectory,
    PreviousSession,
}

impl StartupLocationPolicy {
    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "home" => Some(Self::Home),
            "custom" => Some(Self::CustomDirectory),
            "previous_session" => Some(Self::PreviousSession),
            _ => None,
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::CustomDirectory => "custom",
            Self::PreviousSession => "previous_session",
        }
    }
}

pub(crate) fn apply_toml_startup_config(config: &mut UserConfig, document: &Table) {
    if let Some(value) = toml_string(document, STARTUP_LOCATION_KEY) {
        if let Some(policy) = StartupLocationPolicy::from_config_value(value) {
            config.startup_location_policy = policy;
        }
    }
    if let Some(value) = toml_string(document, STARTUP_CUSTOM_DIRECTORY_KEY) {
        config.startup_custom_directory = PathBuf::from(value);
    }
    if let Some(value) = document
        .get(SAVE_VIEW_STATE_KEY)
        .and_then(toml::Value::as_bool)
    {
        config.save_view_state = value;
    }
}
