use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionRunPosition {
    Single,
    First,
    Middle,
    Last,
}

impl SelectionRunPosition {
    pub(crate) fn from_neighbors(previous_selected: bool, next_selected: bool) -> Self {
        match (previous_selected, next_selected) {
            (false, false) => Self::Single,
            (false, true) => Self::First,
            (true, true) => Self::Middle,
            (true, false) => Self::Last,
        }
    }
}

pub(crate) fn selection_run_position(
    paths: &[PathBuf],
    selected_paths: &HashSet<PathBuf>,
    index: usize,
) -> Option<SelectionRunPosition> {
    let path = paths.get(index)?;
    if !selected_paths.contains(path) {
        return None;
    }

    let previous_selected = index
        .checked_sub(1)
        .and_then(|previous| paths.get(previous))
        .is_some_and(|previous| selected_paths.contains(previous));
    let next_selected = paths
        .get(index + 1)
        .is_some_and(|next| selected_paths.contains(next));
    Some(SelectionRunPosition::from_neighbors(
        previous_selected,
        next_selected,
    ))
}

pub(crate) fn selection_run_position_for_path(
    paths: &[PathBuf],
    selected_paths: &HashSet<PathBuf>,
    path: &Path,
) -> Option<SelectionRunPosition> {
    let index = paths.iter().position(|candidate| candidate == path)?;
    selection_run_position(paths, selected_paths, index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/workspace/{name}"))
    }

    #[test]
    fn adjacent_selection_run_positions_are_classified() {
        let paths = vec![path("a"), path("b"), path("c"), path("d")];
        let selected_paths = HashSet::from([path("a"), path("b"), path("c")]);

        assert_eq!(
            selection_run_position(&paths, &selected_paths, 0),
            Some(SelectionRunPosition::First)
        );
        assert_eq!(
            selection_run_position(&paths, &selected_paths, 1),
            Some(SelectionRunPosition::Middle)
        );
        assert_eq!(
            selection_run_position(&paths, &selected_paths, 2),
            Some(SelectionRunPosition::Last)
        );
        assert_eq!(selection_run_position(&paths, &selected_paths, 3), None);
    }

    #[test]
    fn isolated_selected_path_is_single_run() {
        let paths = vec![path("a"), path("b"), path("c")];
        let selected_paths = HashSet::from([path("b")]);

        assert_eq!(
            selection_run_position(&paths, &selected_paths, 1),
            Some(SelectionRunPosition::Single)
        );
    }
}
