use super::{ShortcutAction, ShortcutRoutingContext};

#[test]
fn application_shortcuts_keep_application_routing() {
    for action in [
        ShortcutAction::Refresh,
        ShortcutAction::Escape,
        ShortcutAction::FocusPathInput,
        ShortcutAction::NavigateBack,
        ShortcutAction::NavigateForward,
        ShortcutAction::NavigateUp,
    ] {
        assert_eq!(
            action.routing_context(),
            ShortcutRoutingContext::Application
        );
    }
}

#[test]
fn file_editing_shortcuts_require_file_content_routing() {
    for action in [
        ShortcutAction::SelectAll,
        ShortcutAction::Copy,
        ShortcutAction::Paste,
        ShortcutAction::Cut,
        ShortcutAction::Delete,
        ShortcutAction::Undo,
        ShortcutAction::Redo,
    ] {
        assert_eq!(
            action.routing_context(),
            ShortcutRoutingContext::FileBrowserContent
        );
    }
}
