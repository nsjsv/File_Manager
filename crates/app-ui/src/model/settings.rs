#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    General,
    ErrorMessages,
    Network,
    FileOperations,
    Search,
    Rendering,
    Shortcuts,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 7] = [
        Self::General,
        Self::ErrorMessages,
        Self::Network,
        Self::FileOperations,
        Self::Search,
        Self::Rendering,
        Self::Shortcuts,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::ErrorMessages => "Error Messages",
            Self::Network => "Network",
            Self::FileOperations => "File Operations",
            Self::Search => "Search",
            Self::Rendering => "Rendering",
            Self::Shortcuts => "Shortcuts",
        }
    }
}
