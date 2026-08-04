use std::path::Path;

use super::PendingOperation;

pub(crate) const CUT_ENTRY_CONTENT_OPACITY: f32 = 0.55;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum FileEntryContentModifier {
    #[default]
    None,
    Cut,
}

impl FileEntryContentModifier {
    pub(crate) fn opacity(self) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Cut => CUT_ENTRY_CONTENT_OPACITY,
        }
    }
}

impl PendingOperation {
    pub(crate) fn content_modifier_for_path(&self, path: &Path) -> FileEntryContentModifier {
        match self {
            Self::Move(paths) if paths.iter().any(|source| source.as_path() == path) => {
                FileEntryContentModifier::Cut
            }
            Self::Copy(_) | Self::Move(_) => FileEntryContentModifier::None,
        }
    }
}
