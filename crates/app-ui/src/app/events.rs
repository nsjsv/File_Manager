use iced::keyboard::{self, key, Key};
use iced::{event, mouse, Event, Theme};

use crate::model::{Message, PathSuggestionDirection};

pub(super) fn global_event_message(event: Event, status: event::Status) -> Option<Message> {
    if let Event::Window(id, window_event) = &event {
        return match window_event {
            iced::window::Event::CloseRequested => {
                Some(Message::AuxiliaryWindowCloseRequested(*id))
            }
            iced::window::Event::Resized { width, height } => {
                Some(Message::AuxiliaryWindowResized(*id, *width, *height))
            }
            _ => None,
        };
    }

    if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = &event {
        return Some(Message::KeyboardModifiersChanged(*modifiers));
    }

    if let Some(message) = path_suggestion_keyboard_message(&event) {
        return Some(message);
    }

    if let Some(message) = keyboard_shortcut_message(&event) {
        return Some(message);
    }

    if let Event::Mouse(mouse::Event::CursorMoved { position }) = &event {
        return Some(Message::CursorMoved(*position));
    }

    if matches!(status, event::Status::Captured) {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = &event {
            return Some(Message::ColumnBrowserWheelScrolled(*delta));
        }
    }

    if matches!(
        &event,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    ) && matches!(status, event::Status::Captured)
    {
        return Some(Message::TabDragFinished);
    }

    if matches!(
        &event,
        Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left | mouse::Button::Right
        ))
    ) && matches!(status, event::Status::Captured)
    {
        return Some(Message::RenameFocusCheckRequested);
    }

    if matches!(status, event::Status::Captured) {
        return None;
    }

    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            Some(Message::DismissFloating)
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::DragSelectionFinished)
        }
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(key::Named::Space),
            ..
        }) => Some(Message::RequestPreview),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Message::Back),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => Some(Message::Forward),
        _ => None,
    }
}

pub(super) fn system_theme() -> Theme {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Theme::Dark,
        Ok(dark_light::Mode::Light) | Ok(dark_light::Mode::Unspecified) | Err(_) => Theme::Light,
    }
}

fn keyboard_shortcut_message(event: &Event) -> Option<Message> {
    let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };

    match key.as_ref() {
        Key::Character("a") | Key::Character("A") if primary_modifier(*modifiers) => {
            Some(Message::PrimarySelectAllRequested)
        }
        Key::Character("c") | Key::Character("C") if primary_modifier(*modifiers) => {
            Some(Message::CopySelected)
        }
        Key::Character("f") | Key::Character("F") if primary_modifier(*modifiers) => {
            Some(Message::SearchOpened)
        }
        Key::Character("v") | Key::Character("V") if primary_modifier(*modifiers) => {
            Some(Message::PastePending)
        }
        Key::Character("x") | Key::Character("X") if primary_modifier(*modifiers) => {
            Some(Message::MoveSelected)
        }
        Key::Named(key::Named::Escape) => Some(Message::DismissFloating),
        Key::Named(key::Named::Copy) => Some(Message::CopySelected),
        Key::Named(key::Named::Paste) => Some(Message::PastePending),
        Key::Named(key::Named::Cut) => Some(Message::MoveSelected),
        Key::Named(key::Named::Delete) => Some(Message::TrashSelected),
        Key::Named(key::Named::Space) if modifiers.alt() => Some(Message::SearchOpened),
        _ => None,
    }
}

fn primary_modifier(modifiers: keyboard::Modifiers) -> bool {
    modifiers.command() || modifiers.control()
}

fn path_suggestion_keyboard_message(event: &Event) -> Option<Message> {
    let Event::Keyboard(keyboard::Event::KeyPressed {
        key: keyboard::Key::Named(named),
        modifiers,
        ..
    }) = event
    else {
        return None;
    };

    match named {
        key::Named::ArrowDown => Some(Message::PathSuggestionMoved(PathSuggestionDirection::Next)),
        key::Named::ArrowUp => Some(Message::PathSuggestionMoved(
            PathSuggestionDirection::Previous,
        )),
        key::Named::Tab if modifiers.shift() => Some(Message::PathSuggestionCompleted(
            PathSuggestionDirection::Previous,
        )),
        key::Named::Tab => Some(Message::PathSuggestionCompleted(
            PathSuggestionDirection::Next,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_shortcut_is_not_blocked_by_captured_widget_event() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("v".into()),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
        });

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::PastePending)));
    }

    #[test]
    fn captured_ctrl_a_requests_focus_aware_select_all() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("a".into()),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
        });

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::PrimarySelectAllRequested)));
    }

    #[test]
    fn ignored_ctrl_a_requests_focus_aware_select_all() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("a".into()),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
        });

        let message = global_event_message(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::PrimarySelectAllRequested)));
    }

    #[test]
    fn captured_left_release_finishes_tab_drag() {
        let event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::TabDragFinished)));
    }

    #[test]
    fn captured_cursor_move_updates_cursor_position() {
        let position = iced::Point::new(12.0, 24.0);
        let event = Event::Mouse(mouse::Event::CursorMoved { position });

        let message = global_event_message(event, event::Status::Captured);

        let Some(Message::CursorMoved(received)) = message else {
            panic!("expected cursor movement message");
        };
        assert_eq!(received.x, position.x);
        assert_eq!(received.y, position.y);
    }

    #[test]
    fn captured_wheel_scroll_reaches_column_browser() {
        let delta = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let event = Event::Mouse(mouse::Event::WheelScrolled { delta });

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::ColumnBrowserWheelScrolled(received)) if received == delta
        ));
    }

    #[test]
    fn ctrl_f_opens_search_even_when_widget_captured_keyboard() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Character("f".into()),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
        });

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::SearchOpened)));
    }

    #[test]
    fn alt_space_opens_search_instead_of_preview() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key::Named::Space),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::ALT,
            text: None,
        });

        let message = global_event_message(event, event::Status::Ignored);

        assert!(matches!(message, Some(Message::SearchOpened)));
    }

    #[test]
    fn escape_dismisses_floating_even_when_widget_captured_keyboard() {
        let event = Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(key::Named::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
        });

        let message = global_event_message(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::DismissFloating)));
    }
}
