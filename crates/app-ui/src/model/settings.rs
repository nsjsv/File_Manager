#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    General,
    Network,
    SearchIndex,
    FileOperations,
    Rendering,
    Shortcuts,
}

impl SettingsCategory {
    pub(crate) const ALL: [Self; 6] = [
        Self::General,
        Self::Network,
        Self::SearchIndex,
        Self::FileOperations,
        Self::Rendering,
        Self::Shortcuts,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Network => "Network",
            Self::SearchIndex => "Search Index",
            Self::FileOperations => "File Operations",
            Self::Rendering => "Rendering",
            Self::Shortcuts => "Shortcuts",
        }
    }
}
