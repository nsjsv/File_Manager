use super::*;
use crate::model::BrowserPaneId;

#[test]
fn icon_grid_wheel_direction_maps_to_zoom_step() {
    assert_eq!(
        icon_grid_zoom_from_wheel(mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }),
        Some(crate::icon_grid_geometry::IconGridZoom::In)
    );
    assert_eq!(
        icon_grid_zoom_from_wheel(mouse::ScrollDelta::Pixels { x: 0.0, y: -2.0 }),
        Some(crate::icon_grid_geometry::IconGridZoom::Out)
    );
}

#[test]
fn ctrl_wheel_on_icon_grid_changes_size_without_starting_scroll() {
    let (mut browser, _) = FileBrowser::new(crate::config::default_user_config());
    let region = ScrollbarRegion::PaneIcons(BrowserPaneId::PRIMARY);
    drop(browser.handle_smooth_scroll_wheel(
        region.clone(),
        mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    ));
    assert!(browser.smooth_scroll.is_active());

    browser.keyboard_modifiers = iced::keyboard::Modifiers::CTRL;
    drop(browser.handle_smooth_scroll_wheel(region, mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }));

    assert_eq!(browser.user_config.icon_grid_size, 112);
    assert!(!browser.smooth_scroll.is_active());
}
