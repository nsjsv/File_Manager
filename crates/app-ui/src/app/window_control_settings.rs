use iced::Task;

use super::FileBrowser;
use crate::model::{Message, WindowChromeLayout, WindowControlKind, WindowControlSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowControlReorderDrag {
    dragged: WindowControlKind,
    side: WindowControlSide,
    target: Option<WindowControlKind>,
}

impl FileBrowser {
    pub(super) fn select_window_chrome_layout(
        &mut self,
        layout: WindowChromeLayout,
    ) -> Task<Message> {
        self.window_control_reorder_drag = None;
        if self.user_config.window_controls.select_layout(layout) {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn toggle_window_control_visibility(
        &mut self,
        kind: WindowControlKind,
    ) -> Task<Message> {
        self.window_control_reorder_drag = None;
        let visibility = self
            .user_config
            .window_controls
            .placement(kind)
            .visibility()
            .toggled();
        if self
            .user_config
            .window_controls
            .set_visibility(kind, visibility)
        {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn select_window_control_side(
        &mut self,
        kind: WindowControlKind,
        side: WindowControlSide,
    ) -> Task<Message> {
        self.window_control_reorder_drag = None;
        if self.user_config.window_controls.move_to_side(kind, side) {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn start_window_control_reorder(
        &mut self,
        kind: WindowControlKind,
    ) -> Task<Message> {
        self.window_control_reorder_drag = Some(WindowControlReorderDrag {
            dragged: kind,
            side: self.user_config.window_controls.placement(kind).side(),
            target: None,
        });
        Task::none()
    }

    pub(super) fn enter_window_control_reorder_target(
        &mut self,
        target: WindowControlKind,
    ) -> Task<Message> {
        let Some(drag) = self.window_control_reorder_drag.as_mut() else {
            return Task::none();
        };
        let target_side = self.user_config.window_controls.placement(target).side();
        drag.target = (target != drag.dragged && target_side == drag.side).then_some(target);
        Task::none()
    }

    pub(super) fn exit_window_control_reorder_target(
        &mut self,
        target: WindowControlKind,
    ) -> Task<Message> {
        if let Some(drag) = self.window_control_reorder_drag.as_mut() {
            if drag.target == Some(target) {
                drag.target = None;
            }
        }
        Task::none()
    }

    pub(super) fn finish_window_control_reorder(&mut self) -> Task<Message> {
        let Some(drag) = self.window_control_reorder_drag.take() else {
            return Task::none();
        };
        let Some(target) = drag.target else {
            return Task::none();
        };
        if self
            .user_config
            .window_controls
            .move_before_on_same_side(drag.dragged, target)
        {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn reset_window_controls(&mut self) -> Task<Message> {
        self.window_control_reorder_drag = None;
        if self.user_config.window_controls.reset() {
            self.persist_user_preferences_command()
        } else {
            Task::none()
        }
    }

    pub(super) fn cancel_window_control_reorder(&mut self) {
        self.window_control_reorder_drag = None;
    }

    pub(crate) fn dragged_window_control(&self) -> Option<WindowControlKind> {
        self.window_control_reorder_drag
            .as_ref()
            .map(|drag| drag.dragged)
    }

    pub(crate) fn window_control_reorder_target(&self) -> Option<WindowControlKind> {
        self.window_control_reorder_drag
            .as_ref()
            .and_then(|drag| drag.target)
    }
}

#[cfg(test)]
mod tests {
    use iced_runtime::task::into_stream;

    use super::*;
    use crate::config;
    use crate::model::WindowControlVisibility;

    fn kinds_on(browser: &FileBrowser, side: WindowControlSide) -> Vec<WindowControlKind> {
        browser
            .user_config()
            .window_controls
            .placements_on(side)
            .map(|placement| placement.kind())
            .collect()
    }

    #[test]
    fn layout_visibility_and_side_changes_persist_only_real_updates() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        assert!(into_stream(
            browser.select_window_chrome_layout(WindowChromeLayout::SeparateTitleBar)
        )
        .is_some());
        drop(browser.accept_user_preferences_saved(Ok(())));
        assert!(into_stream(
            browser.select_window_chrome_layout(WindowChromeLayout::SeparateTitleBar)
        )
        .is_none());
        assert_eq!(
            browser.user_config().window_controls.layout(),
            WindowChromeLayout::SeparateTitleBar
        );
        assert!(
            into_stream(browser.toggle_window_control_visibility(WindowControlKind::Minimize))
                .is_some()
        );
        drop(browser.accept_user_preferences_saved(Ok(())));
        assert_eq!(
            browser
                .user_config()
                .window_controls
                .placement(WindowControlKind::Minimize)
                .visibility(),
            WindowControlVisibility::Hidden
        );
        assert!(into_stream(
            browser.select_window_control_side(WindowControlKind::Close, WindowControlSide::Left)
        )
        .is_some());
        drop(browser.accept_user_preferences_saved(Ok(())));
        assert!(into_stream(
            browser.select_window_control_side(WindowControlKind::Close, WindowControlSide::Left)
        )
        .is_none());
    }

    #[test]
    fn close_visibility_toggle_is_rejected_without_persistence() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        assert!(
            into_stream(browser.toggle_window_control_visibility(WindowControlKind::Close))
                .is_none()
        );
        assert!(browser
            .user_config()
            .window_controls
            .placement(WindowControlKind::Close)
            .visibility()
            .is_visible());
    }

    #[test]
    fn reorder_commits_once_on_release_and_rejects_cross_side_target() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        drop(browser.select_window_control_side(WindowControlKind::Close, WindowControlSide::Left));
        drop(browser.accept_user_preferences_saved(Ok(())));
        drop(browser.start_window_control_reorder(WindowControlKind::Minimize));
        drop(browser.enter_window_control_reorder_target(WindowControlKind::Close));
        assert_eq!(browser.window_control_reorder_target(), None);
        assert!(into_stream(browser.finish_window_control_reorder()).is_none());

        drop(
            browser
                .select_window_control_side(WindowControlKind::Minimize, WindowControlSide::Left),
        );
        drop(browser.accept_user_preferences_saved(Ok(())));
        drop(browser.start_window_control_reorder(WindowControlKind::Minimize));
        drop(browser.enter_window_control_reorder_target(WindowControlKind::Close));
        assert_eq!(
            browser.window_control_reorder_target(),
            Some(WindowControlKind::Close)
        );
        assert!(into_stream(browser.finish_window_control_reorder()).is_some());
        drop(browser.accept_user_preferences_saved(Ok(())));
        assert_eq!(
            kinds_on(&browser, WindowControlSide::Left),
            vec![WindowControlKind::Minimize, WindowControlKind::Close]
        );
        assert!(into_stream(browser.finish_window_control_reorder()).is_none());
    }

    #[test]
    fn settings_lifecycle_clears_reorder_drag() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.start_window_control_reorder(WindowControlKind::Minimize));
        assert_eq!(
            browser.dragged_window_control(),
            Some(WindowControlKind::Minimize)
        );
        drop(browser.select_settings_category(crate::model::SettingsCategory::Logs));
        assert_eq!(browser.dragged_window_control(), None);

        drop(browser.start_window_control_reorder(WindowControlKind::Close));
        drop(browser.close_settings_window());
        assert_eq!(browser.dragged_window_control(), None);

        drop(browser.start_window_control_reorder(WindowControlKind::Close));
        drop(browser.enter_window_control_reorder_target(WindowControlKind::Minimize));
        drop(browser.finish_pointer_drag_interactions());
        assert_eq!(
            kinds_on(&browser, WindowControlSide::Right),
            vec![
                WindowControlKind::Close,
                WindowControlKind::Minimize,
                WindowControlKind::MaximizeRestore,
            ]
        );
        assert_eq!(browser.dragged_window_control(), None);

        drop(browser.start_window_control_reorder(WindowControlKind::MaximizeRestore));
        browser.clear_pointer_driven_interaction_state();
        assert_eq!(browser.dragged_window_control(), None);
    }

    #[test]
    fn reset_restores_confirmed_default() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        drop(browser.select_window_chrome_layout(WindowChromeLayout::SeparateTitleBar));
        drop(browser.select_window_control_side(WindowControlKind::Close, WindowControlSide::Left));
        drop(browser.accept_user_preferences_saved(Ok(())));
        drop(browser.accept_user_preferences_saved(Ok(())));

        assert!(into_stream(browser.reset_window_controls()).is_some());
        assert_eq!(
            browser.user_config().window_controls,
            crate::model::WindowControlsConfig::default()
        );
        assert!(into_stream(browser.reset_window_controls()).is_none());
    }
}
