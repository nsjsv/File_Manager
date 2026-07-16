use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::Task;

use super::FileBrowser;
use crate::commands::load_directory_command;
use crate::config::StartupLocationPolicy;
use crate::model::{
    BrowserPane, BrowserPaneId, BrowserPaneLayout, BrowserPaneSession, BrowserSessionSnapshot,
    BrowserTabSession, ColumnBrowserViewport, Message,
};
use crate::thumbnail_cache::ColumnViewport;

impl FileBrowser {
    pub(super) fn startup_session_plan(
        &self,
        home: &Path,
        session: Option<BrowserSessionSnapshot>,
    ) -> StartupSessionPlan {
        match self.user_config.startup_location_policy {
            StartupLocationPolicy::Home => StartupSessionPlan::Directory {
                directory: home.to_path_buf(),
                error: None,
            },
            StartupLocationPolicy::CustomDirectory => {
                self.startup_directory_plan(&self.user_config.startup_custom_directory, home)
            }
            StartupLocationPolicy::PreviousSession => match session {
                Some(session) => StartupSessionPlan::Session(session),
                None => StartupSessionPlan::Directory {
                    directory: home.to_path_buf(),
                    error: Some(
                        "No saved view state was found; opening the home directory.".to_owned(),
                    ),
                },
            },
        }
    }

    fn startup_directory_plan(&self, candidate: &Path, home: &Path) -> StartupSessionPlan {
        if directory_is_usable(candidate) {
            return StartupSessionPlan::Directory {
                directory: candidate.to_path_buf(),
                error: None,
            };
        }
        StartupSessionPlan::Directory {
            directory: home.to_path_buf(),
            error: Some(format!(
                "Could not open startup directory {}; opening the home directory.",
                candidate.to_string_lossy()
            )),
        }
    }

    pub(super) fn apply_startup_session_plan(
        &mut self,
        plan: StartupSessionPlan,
        home: &Path,
    ) -> Task<Message> {
        match plan {
            StartupSessionPlan::Directory { directory, error } => {
                self.apply_single_startup_directory(directory, error)
            }
            StartupSessionPlan::Session(session) => self.apply_restored_session(session, home),
        }
    }

    pub(super) fn fallback_startup_directory_after_session_store_error(
        &mut self,
        home: &Path,
        error: String,
    ) -> Task<Message> {
        self.apply_single_startup_directory(
            home.to_path_buf(),
            Some(format!(
                "Failed to restore saved view state: {error}; opening the home directory."
            )),
        )
    }

    fn apply_single_startup_directory(
        &mut self,
        directory: PathBuf,
        error: Option<String>,
    ) -> Task<Message> {
        self.current_dir = directory.clone();
        self.is_trash_view = false;
        self.entries.clear();
        self.directory_loading_placeholder_entries.clear();
        self.trash_entries.clear();
        self.deepest_open_column_directory = None;
        self.expanded_directories.clear();
        self.column_browser_viewport = ColumnBrowserViewport::default();
        self.column_viewports.clear();
        self.icon_grid_viewports.clear();
        self.back_stack.clear();
        self.forward_stack.clear();
        self.is_loading = true;
        self.replace_global_error(error);
        self.tabs = vec![{
            let mut tab = crate::model::BrowserTab::directory(0, directory.clone());
            tab.view_mode = self.view_mode;
            tab
        }];
        self.active_tab_id = 0;
        self.pane_layout = BrowserPaneLayout::Single {
            active: BrowserPaneId::PRIMARY,
        };
        self.panes = vec![self.capture_active_pane_snapshot()];
        let request = self.next_directory_load_request(directory);
        let cancellation = self.directory_load_cancellation(&request);
        load_directory_command(request, self.options.clone(), cancellation)
    }

    fn apply_restored_session(
        &mut self,
        session: BrowserSessionSnapshot,
        home: &Path,
    ) -> Task<Message> {
        let BrowserSessionSnapshot { panes, layout } = session;
        let restored_panes = panes
            .into_iter()
            .filter_map(restored_pane_from_session)
            .collect::<Vec<_>>();
        if restored_panes.is_empty() {
            return self.apply_single_startup_directory(
                home.to_path_buf(),
                Some(
                    "Saved view state could not be restored; opening the home directory."
                        .to_owned(),
                ),
            );
        }
        self.panes = restored_panes.clone();
        self.icon_grid_viewports.clear();
        self.pane_layout = sanitize_layout(layout, &restored_panes);
        let active_pane_id = self.pane_layout.active();
        let active_pane = restored_panes
            .iter()
            .find(|pane| pane.id == active_pane_id)
            .or_else(|| restored_panes.first())
            .cloned()
            .expect("restored panes is non-empty");
        self.pane_layout = self.pane_layout.with_active(active_pane.id);
        self.restore_pane_snapshot(active_pane);
        self.is_loading = true;
        self.clear_global_error();
        self.sync_active_pane_state();

        Task::batch([
            self.reload_visible_panes(),
            self.restore_visible_column_browser_viewports(),
        ])
    }
}

pub(super) enum StartupSessionPlan {
    Directory {
        directory: PathBuf,
        error: Option<String>,
    },
    Session(BrowserSessionSnapshot),
}

fn restored_pane_from_session(pane: BrowserPaneSession) -> Option<BrowserPane> {
    let restored_tabs = pane
        .tabs
        .into_iter()
        .filter(|tab| tab.is_trash_view || directory_is_usable(&tab.directory))
        .collect::<Vec<_>>();
    if restored_tabs.is_empty() {
        return None;
    }
    let active_tab = restored_tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .or_else(|| restored_tabs.first())
        .cloned()
        .expect("restored tabs is non-empty");
    let tabs = restored_tabs
        .iter()
        .map(BrowserTabSession::to_browser_tab)
        .collect::<Vec<_>>();
    let active_expanded_directories = active_tab.restored_expanded_directories();
    let mut browser_pane = BrowserPane {
        id: pane.id,
        current_dir: active_tab.directory.clone(),
        is_trash_view: active_tab.is_trash_view,
        entries: Vec::new(),
        directory_loading_placeholder_entries: Vec::new(),
        trash_entries: Vec::new(),
        selected: active_tab.selected,
        selected_paths: active_tab.selected_paths,
        selection_anchor: None,
        deepest_open_column_directory: active_tab.deepest_open_column_directory,
        expanded_directories: active_expanded_directories,
        view_mode: active_tab.view_mode,
        column_browser_viewport: sanitize_column_browser_viewport(pane.column_browser_viewport),
        column_viewports: sanitize_column_viewports(pane.column_viewports),
        tabs,
        active_tab_id: active_tab.id,
        directory_load_generation: 0,
        directory_load_cancel: None,
        back_stack: active_tab.back_stack,
        forward_stack: active_tab.forward_stack,
        is_loading: true,
    };
    browser_pane.sync_active_tab_state();
    Some(browser_pane)
}

fn sanitize_column_browser_viewport(viewport: ColumnBrowserViewport) -> ColumnBrowserViewport {
    if viewport.offset_x.is_finite() && viewport.width.is_finite() {
        return ColumnBrowserViewport {
            offset_x: viewport.offset_x.max(0.0),
            width: viewport.width.max(1.0),
        };
    }
    ColumnBrowserViewport::default()
}

fn sanitize_column_viewports(
    viewports: HashMap<PathBuf, ColumnViewport>,
) -> HashMap<PathBuf, ColumnViewport> {
    viewports
        .into_iter()
        .filter(|(_, viewport)| viewport.offset_y.is_finite() && viewport.height.is_finite())
        .collect()
}

fn sanitize_layout(layout: BrowserPaneLayout, panes: &[BrowserPane]) -> BrowserPaneLayout {
    let exists = |id: BrowserPaneId| panes.iter().any(|pane| pane.id == id);
    match layout {
        BrowserPaneLayout::Single { active } if exists(active) => {
            BrowserPaneLayout::Single { active }
        }
        BrowserPaneLayout::Split {
            axis,
            first,
            second,
            active,
        } if exists(first) && exists(second) && exists(active) => BrowserPaneLayout::Split {
            axis,
            first,
            second,
            active,
        },
        _ => BrowserPaneLayout::Single {
            active: panes[0].id,
        },
    }
}

fn directory_is_usable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}
