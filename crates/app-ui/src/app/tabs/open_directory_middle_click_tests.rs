use std::path::PathBuf;

use iced::keyboard;

use super::*;
use crate::config;

fn browser() -> FileBrowser {
    FileBrowser::new(config::default_user_config()).0
}

#[test]
fn plain_middle_click_keeps_opening_directory_in_new_tab() {
    let mut browser = browser();
    let directory = PathBuf::from("/workspace/project");

    drop(browser.open_directory_from_middle_click(directory.clone()));

    assert!(matches!(
        browser.pane_layout,
        BrowserPaneLayout::Single {
            active: BrowserPaneId::PRIMARY
        }
    ));
    assert_eq!(browser.tabs.len(), 2);
    assert_eq!(browser.current_dir, directory);
}

#[test]
fn shift_middle_click_opens_directory_in_horizontal_split() {
    let mut browser = browser();
    let directory = PathBuf::from("/workspace/project");
    browser.keyboard_modifiers = keyboard::Modifiers::SHIFT;

    drop(browser.open_directory_from_middle_click(directory.clone()));

    assert_split_directory(&browser, directory, SplitAxis::Horizontal);
}

#[test]
fn control_middle_click_opens_directory_in_vertical_split() {
    let mut browser = browser();
    let directory = PathBuf::from("/workspace/project");
    browser.keyboard_modifiers = keyboard::Modifiers::CTRL;

    drop(browser.open_directory_from_middle_click(directory.clone()));

    assert_split_directory(&browser, directory, SplitAxis::Vertical);
}

fn assert_split_directory(browser: &FileBrowser, directory: PathBuf, expected_axis: SplitAxis) {
    let BrowserPaneLayout::Split {
        axis,
        first,
        second,
        active,
    } = browser.pane_layout
    else {
        panic!("expected split pane layout");
    };

    assert_eq!(axis, expected_axis);
    assert_eq!(first, BrowserPaneId::PRIMARY);
    assert_eq!(active, second);
    assert_eq!(browser.current_dir, directory);
    assert_eq!(
        browser
            .pane_by_id(second)
            .expect("split destination pane")
            .current_dir,
        directory
    );
}
