use std::path::PathBuf;

use desktop_linux::WaylandFileDragSessionId;
use iced::Point;

use super::super::sidebar_bookmarks::{
    sidebar_bookmark_row_pointer_target, SidebarBookmarkRowPointerTarget,
};
use super::super::FileBrowser;
use super::drag::safe_file_drag_target;
use crate::model::{
    FileDragTarget, SidebarBookmarkDropSlot, SidebarFileDragTargetBounds,
    WaylandFileDragHitTestBounds,
};

impl FileBrowser {
    fn wayland_file_drag_target_at_position(
        &self,
        session_id: WaylandFileDragSessionId,
        position: Point,
    ) -> Option<(FileDragTarget, Option<PathBuf>)> {
        let file_drag = self.file_drag.as_ref()?;
        let snapshot = file_drag
            .wayland_target
            .as_ref()
            .filter(|snapshot| snapshot.session_id == session_id)?;
        let resolved = if snapshot.bookmark_source.is_some() {
            directory_file_drag_sidebar_target_in_snapshot(position, &snapshot.hit_test_bounds)
        } else {
            file_drag_sidebar_directory_in_bounds(
                position,
                &snapshot.hit_test_bounds.sidebar_directories,
            )
        }
        .map(WaylandFileDragResolvedTarget::Sidebar)
        .or_else(|| {
            file_drag_directory_target_in_snapshot(position, &snapshot.hit_test_bounds)
                .map(WaylandFileDragResolvedTarget::Content)
        })?;
        let (target, hovered_entry) = match resolved {
            WaylandFileDragResolvedTarget::Sidebar(target) => (target, None),
            WaylandFileDragResolvedTarget::Content(target) => (
                FileDragTarget::Directory(target.directory),
                target.hovered_entry,
            ),
        };
        safe_file_drag_target(&file_drag.sources, Some(target))
            .map(|target| (target, hovered_entry))
    }

    pub(in crate::app) fn refresh_wayland_file_drag_target_at_position(
        &mut self,
        session_id: WaylandFileDragSessionId,
        position: Point,
    ) -> Option<FileDragTarget> {
        let resolved = self.wayland_file_drag_target_at_position(session_id, position);
        let (target, hovered_entry) = resolved
            .map(|(target, hovered_entry)| (Some(target), hovered_entry))
            .unwrap_or((None, None));

        self.hovered_entry = hovered_entry;
        let sidebar_contains_position = self
            .file_drag
            .as_ref()
            .and_then(|drag| drag.wayland_target.as_ref())
            .is_some_and(|snapshot| {
                snapshot
                    .hit_test_bounds
                    .sidebar_directories
                    .iter()
                    .any(|target| target.bounds.contains(position))
                    || snapshot
                        .hit_test_bounds
                        .empty_sidebar_bookmarks
                        .is_some_and(|bounds| bounds.contains(position))
            });
        self.hovered_sidebar = match target.as_ref() {
            Some(FileDragTarget::Directory(directory)) if sidebar_contains_position => {
                Some(directory.clone())
            }
            _ => None,
        };
        self.cursor_paste_directory = match target.as_ref() {
            Some(FileDragTarget::Directory(directory)) if !sidebar_contains_position => {
                Some(directory.clone())
            }
            _ => None,
        };
        self.sidebar_bookmark_drop_slot = match target.as_ref() {
            Some(FileDragTarget::SidebarBookmarkSlot(slot)) => Some(*slot),
            _ => None,
        };
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = target.clone();
            if let Some(snapshot) = &mut file_drag.wayland_target {
                snapshot.position = Some(position);
                snapshot.target = target.clone();
            }
        }
        target
    }

    pub(in crate::app) fn wayland_file_drag_target_at_drop(
        &self,
        session_id: WaylandFileDragSessionId,
        position: Point,
    ) -> Option<FileDragTarget> {
        self.wayland_file_drag_target_at_position(session_id, position)
            .map(|(target, _)| target)
    }

    pub(in crate::app) fn clear_wayland_file_drag_highlight(&mut self) {
        self.hovered_entry = None;
        self.hovered_sidebar = None;
        self.cursor_paste_directory = None;
        self.sidebar_bookmark_drop_slot = None;
        if let Some(file_drag) = &mut self.file_drag {
            file_drag.target = None;
        }
    }

    pub(in crate::app) fn clear_wayland_file_drag_target(&mut self) {
        self.clear_wayland_file_drag_highlight();
        if let Some(file_drag) = &mut self.file_drag {
            if let Some(snapshot) = &mut file_drag.wayland_target {
                snapshot.position = None;
                snapshot.target = None;
            }
        }
    }
}

enum WaylandFileDragResolvedTarget {
    Sidebar(FileDragTarget),
    Content(WaylandFileDragDirectoryTarget),
}

struct WaylandFileDragDirectoryTarget {
    directory: PathBuf,
    hovered_entry: Option<PathBuf>,
}

fn file_drag_directory_target_in_snapshot(
    position: Point,
    hit_test_bounds: &WaylandFileDragHitTestBounds,
) -> Option<WaylandFileDragDirectoryTarget> {
    if let Some(directory) = hit_test_bounds
        .breadcrumbs
        .iter()
        .filter(|target| {
            target.item_bounds.contains(position)
                && target.viewport_bounds.contains(position)
                && hit_test_bounds
                    .directory_targets
                    .iter()
                    .any(|target_bounds| target_bounds.pane_id == target.pane_id)
        })
        .max_by_key(|target| target.directory.components().count())
        .map(|target| target.directory.clone())
    {
        return Some(WaylandFileDragDirectoryTarget {
            directory,
            hovered_entry: None,
        });
    }
    if hit_test_bounds
        .blocked_directories
        .iter()
        .rev()
        .any(|blocked| blocked.bounds.contains(position))
    {
        return None;
    }
    for entry_bounds in hit_test_bounds.entries.iter().rev() {
        if entry_bounds.bounds.contains(position) {
            return Some(WaylandFileDragDirectoryTarget {
                directory: entry_bounds.directory.clone(),
                hovered_entry: Some(entry_bounds.path.clone()),
            });
        }
    }
    hit_test_bounds
        .directory_targets
        .iter()
        .rev()
        .find(|target_bounds| target_bounds.bounds.contains(position))
        .map(|target_bounds| WaylandFileDragDirectoryTarget {
            directory: target_bounds.directory.clone(),
            hovered_entry: None,
        })
}

fn file_drag_sidebar_directory_in_bounds(
    position: Point,
    sidebar_bounds: &[SidebarFileDragTargetBounds],
) -> Option<FileDragTarget> {
    sidebar_bounds
        .iter()
        .rev()
        .find(|target| target.bounds.contains(position))
        .map(|target| FileDragTarget::Directory(target.directory.clone()))
}

fn directory_file_drag_sidebar_target_in_snapshot(
    position: Point,
    hit_test_bounds: &WaylandFileDragHitTestBounds,
) -> Option<FileDragTarget> {
    if hit_test_bounds
        .empty_sidebar_bookmarks
        .is_some_and(|bounds| bounds.contains(position))
    {
        return Some(FileDragTarget::SidebarBookmarkSlot(
            SidebarBookmarkDropSlot::Insert { index: 0 },
        ));
    }
    directory_file_drag_sidebar_target_in_bounds(position, &hit_test_bounds.sidebar_directories)
}

fn directory_file_drag_sidebar_target_in_bounds(
    position: Point,
    sidebar_bounds: &[SidebarFileDragTargetBounds],
) -> Option<FileDragTarget> {
    let target = sidebar_bounds
        .iter()
        .rev()
        .find(|target| target.bounds.contains(position))?;
    let Some(favorite_index) = target.favorite_index else {
        return Some(FileDragTarget::Directory(target.directory.clone()));
    };
    match sidebar_bookmark_row_pointer_target(position.y, target.bounds.y, target.bounds.height) {
        SidebarBookmarkRowPointerTarget::InsertBefore => Some(FileDragTarget::SidebarBookmarkSlot(
            SidebarBookmarkDropSlot::Insert {
                index: favorite_index,
            },
        )),
        SidebarBookmarkRowPointerTarget::Directory => {
            Some(FileDragTarget::Directory(target.directory.clone()))
        }
        SidebarBookmarkRowPointerTarget::InsertAfter => Some(FileDragTarget::SidebarBookmarkSlot(
            SidebarBookmarkDropSlot::Insert {
                index: favorite_index + 1,
            },
        )),
    }
}

#[cfg(test)]
mod tests {
    use iced::{Rectangle, Size};

    use super::*;

    #[test]
    fn empty_sidebar_bookmarks_snapshot_resolves_first_insert_slot() {
        let hit_test_bounds = WaylandFileDragHitTestBounds {
            empty_sidebar_bookmarks: Some(Rectangle::new(
                Point::new(10.0, 40.0),
                Size::new(180.0, 24.0),
            )),
            ..WaylandFileDragHitTestBounds::default()
        };
        assert_eq!(
            directory_file_drag_sidebar_target_in_snapshot(
                Point::new(20.0, 50.0),
                &hit_test_bounds
            ),
            Some(FileDragTarget::SidebarBookmarkSlot(
                SidebarBookmarkDropSlot::Insert { index: 0 }
            ))
        );
    }

    #[test]
    fn column_blank_targets_remain_bound_to_each_rendered_directory() {
        let pane_id = crate::model::BrowserPaneId::PRIMARY;
        let workspace = PathBuf::from("/workspace");
        let project = workspace.join("project");
        let hit_test_bounds = WaylandFileDragHitTestBounds {
            directory_targets: vec![
                crate::model::DirectoryFileDragTargetBounds {
                    pane_id,
                    directory: workspace,
                    bounds: Rectangle::new(Point::new(0.0, 50.0), Size::new(200.0, 400.0)),
                },
                crate::model::DirectoryFileDragTargetBounds {
                    pane_id,
                    directory: project.clone(),
                    bounds: Rectangle::new(Point::new(205.0, 50.0), Size::new(200.0, 400.0)),
                },
            ],
            ..WaylandFileDragHitTestBounds::default()
        };

        let target =
            file_drag_directory_target_in_snapshot(Point::new(300.0, 300.0), &hit_test_bounds)
                .expect("second column blank target");

        assert_eq!(target.directory, project);
        assert!(target.hovered_entry.is_none());
    }

    #[test]
    fn blocked_animation_band_prevents_outer_directory_fallback() {
        let pane_id = crate::model::BrowserPaneId::PRIMARY;
        let hit_test_bounds = WaylandFileDragHitTestBounds {
            directory_targets: vec![crate::model::DirectoryFileDragTargetBounds {
                pane_id,
                directory: PathBuf::from("/workspace"),
                bounds: Rectangle::new(Point::ORIGIN, Size::new(500.0, 500.0)),
            }],
            blocked_directories: vec![crate::model::FileDragBlockedDirectoryBounds {
                pane_id,
                bounds: Rectangle::new(Point::new(0.0, 120.0), Size::new(500.0, 120.0)),
            }],
            ..WaylandFileDragHitTestBounds::default()
        };

        assert!(
            file_drag_directory_target_in_snapshot(Point::new(200.0, 180.0), &hit_test_bounds,)
                .is_none()
        );
        assert_eq!(
            file_drag_directory_target_in_snapshot(Point::new(200.0, 300.0), &hit_test_bounds)
                .map(|target| target.directory),
            Some(PathBuf::from("/workspace")),
        );
    }

    #[test]
    fn sidebar_snapshot_resolves_directory_and_insert_edges() {
        let directory = PathBuf::from("/workspace/projects");
        let bounds = vec![SidebarFileDragTargetBounds {
            directory: directory.clone(),
            favorite_index: Some(2),
            bounds: Rectangle::new(Point::new(0.0, 40.0), Size::new(200.0, 32.0)),
        }];

        assert_eq!(
            directory_file_drag_sidebar_target_in_bounds(Point::new(20.0, 56.0), &bounds),
            Some(FileDragTarget::Directory(directory))
        );
        assert_eq!(
            directory_file_drag_sidebar_target_in_bounds(Point::new(20.0, 42.0), &bounds),
            Some(FileDragTarget::SidebarBookmarkSlot(
                SidebarBookmarkDropSlot::Insert { index: 2 }
            ))
        );
        assert_eq!(
            directory_file_drag_sidebar_target_in_bounds(Point::new(20.0, 70.0), &bounds),
            Some(FileDragTarget::SidebarBookmarkSlot(
                SidebarBookmarkDropSlot::Insert { index: 3 }
            ))
        );
        assert_eq!(
            file_drag_sidebar_directory_in_bounds(Point::new(20.0, 42.0), &bounds),
            Some(FileDragTarget::Directory(PathBuf::from(
                "/workspace/projects"
            )))
        );
    }
}
