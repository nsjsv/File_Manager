#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    General,
    ErrorMessages,
    Network,
    SearchIndex,
    FileOperations,
    Rendering,
    Shortcuts,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 7] = [
        Self::General,
        Self::ErrorMessages,
        Self::Network,
        Self::SearchIndex,
        Self::FileOperations,
        Self::Rendering,
        Self::Shortcuts,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::ErrorMessages => "Error Messages",
            Self::Network => "Network",
            Self::SearchIndex => "Search Index",
            Self::FileOperations => "File Operations",
            Self::Rendering => "Rendering",
            Self::Shortcuts => "Shortcuts",
        }
    }
}
