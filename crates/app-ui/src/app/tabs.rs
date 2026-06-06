use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::Task;

use super::FileBrowser;
use crate::model::{trash_location_path, BrowserTab, Message, NavigationMode};

impl FileBrowser {
    pub(super) fn sync_active_tab_state(&mut self) {
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab_id)
        else {
            return;
        };

        tab.directory = self.current_dir.clone();
        tab.is_trash_view = self.is_trash_view;
        tab.entries = self.entries.clone();
        tab.trash_entries = self.trash_entries.clone();
        tab.selected = self.selected.clone();
        tab.selected_paths = self.selected_paths.clone();
        tab.selection_anchor = self.selection_anchor.clone();
        tab.expanded_directories = self.expanded_directories.clone();
        tab.back_stack = self.back_stack.clone();
        tab.forward_stack = self.forward_stack.clone();
    }

    pub(super) fn open_directory_in_new_tab(&mut self, directory: PathBuf) -> Task<Message> {
        self.sync_active_tab_state();
        self.context_menu = None;
        self.clear_preview();
        self.is_column_view_settings_open = false;

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(BrowserTab {
            id: tab_id,
            directory: directory.clone(),
            is_trash_view: false,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            expanded_directories: HashMap::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        });

        self.active_tab_id = tab_id;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.navigate_to(directory, NavigationMode::KeepHistory)
    }

    pub(super) fn open_trash_in_new_tab(&mut self) -> Task<Message> {
        self.sync_active_tab_state();
        self.context_menu = None;
        self.clear_preview();
        self.is_column_view_settings_open = false;

        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(BrowserTab {
            id: tab_id,
            directory: trash_location_path(),
            is_trash_view: true,
            entries: Vec::new(),
            trash_entries: Vec::new(),
            selected: None,
            selected_paths: HashSet::new(),
            selection_anchor: None,
            expanded_directories: HashMap::new(),
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
        });

        self.active_tab_id = tab_id;
        self.back_stack.clear();
        self.forward_stack.clear();
        self.open_trash_view(NavigationMode::KeepHistory)
    }

    pub(super) fn select_tab(&mut self, tab_id: usize) -> Task<Message> {
        if tab_id == self.active_tab_id {
            self.sync_active_tab_state();
            return Task::none();
        }

        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id).cloned() else {
            return Task::none();
        };

        self.sync_active_tab_state();
        self.active_tab_id = tab.id;
        self.is_trash_view = tab.is_trash_view;
        self.entries = tab.entries;
        self.trash_entries = tab.trash_entries;
        self.selected = tab.selected;
        self.selected_paths = tab.selected_paths;
        self.selection_anchor = tab.selection_anchor;
        self.expanded_directories = tab.expanded_directories;
        self.back_stack = tab.back_stack;
        self.forward_stack = tab.forward_stack;
        self.current_dir = tab.directory;
        self.reload_current()
    }

    pub(super) fn close_tab(&mut self, tab_id: usize) -> Task<Message> {
        if self.tabs.len() == 1 {
            return Task::none();
        }

        let Some(closing_index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Task::none();
        };
        let was_active = tab_id == self.active_tab_id;
        self.tabs.remove(closing_index);

        if self.tab_drag_id == Some(tab_id) {
            self.finish_tab_drag();
        }

        if !was_active {
            return Task::none();
        }

        let adjacent_index = closing_index.min(self.tabs.len() - 1);
        let adjacent_tab_id = self.tabs[adjacent_index].id;
        self.select_tab(adjacent_tab_id)
    }

    pub(super) fn start_tab_drag(&mut self, tab_id: usize) {
        if self.tabs.iter().any(|tab| tab.id == tab_id) {
            self.tab_drag_id = Some(tab_id);
        }
    }

    pub(super) fn reorder_dragged_tab(&mut self, entered_tab_id: usize) {
        let Some(dragged_tab_id) = self.tab_drag_id else {
            return;
        };
        if dragged_tab_id == entered_tab_id {
            return;
        }

        let Some(dragged_index) = self.tabs.iter().position(|tab| tab.id == dragged_tab_id) else {
            self.finish_tab_drag();
            return;
        };
        let Some(entered_index) = self.tabs.iter().position(|tab| tab.id == entered_tab_id) else {
            return;
        };

        let dragged_tab = self.tabs.remove(dragged_index);
        self.tabs.insert(entered_index, dragged_tab);
    }

    pub(super) fn finish_tab_drag(&mut self) {
        self.tab_drag_id = None;
    }
}
