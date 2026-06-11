use iced::keyboard;
use iced::{event, mouse, window, Event, Theme};

use crate::model::Message;

pub(super) fn global_event_message(
    event: Event,
    status: event::Status,
    window: window::Id,
) -> Option<Message> {
    if let Event::Window(window_event) = &event {
        return match window_event {
            iced::window::Event::CloseRequested => {
                Some(Message::AuxiliaryWindowCloseRequested(window))
            }
            iced::window::Event::Resized(size) => Some(Message::AuxiliaryWindowResized(
                window,
                size.width,
                size.height,
            )),
            iced::window::Event::Focused => Some(Message::WindowFocused(window)),
            iced::window::Event::Unfocused => Some(Message::WindowUnfocused(window)),
            _ => None,
        };
    }

    if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = &event {
        return Some(Message::KeyboardModifiersChanged(*modifiers));
    }

    if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = &event {
        return Some(Message::KeyboardKeyPressed {
            key: key.clone(),
            modifiers: *modifiers,
            status,
        });
    }

    if let Event::Mouse(mouse::Event::CursorMoved { position }) = &event {
        return Some(Message::CursorMoved(*position));
    }

    if let Some(message) = pointer_pressed_message(&event, status) {
        return Some(message);
    }

    if matches!(status, event::Status::Captured) {
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = &event {
            return Some(Message::CapturedWheelScrolled(*delta));
        }
    }

    if matches!(
        &event,
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    ) && matches!(status, event::Status::Captured)
    {
        return Some(Message::TabDragFinished);
    }

    if matches!(status, event::Status::Captured) {
        return None;
    }

    match event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::DragSelectionFinished)
        }
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Message::Back),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => Some(Message::Forward),
        _ => None,
    }
}

fn pointer_pressed_message(event: &Event, status: event::Status) -> Option<Message> {
    let Event::Mouse(mouse::Event::ButtonPressed(button)) = event else {
        return None;
    };

    match button {
        mouse::Button::Left | mouse::Button::Right | mouse::Button::Middle => {
            Some(Message::WindowPointerPressed {
                button: *button,
                status,
            })
        }
        _ => None,
    }
}

pub(super) fn system_theme() -> Theme {
    match dark_light::detect() {
        Ok(dark_light::Mode::Dark) => Theme::Dark,
        Ok(dark_light::Mode::Light) | Ok(dark_light::Mode::Unspecified) | Err(_) => Theme::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{self, key, Key};

    fn route_event(event: Event, status: event::Status) -> Option<Message> {
        global_event_message(event, status, iced::window::Id::unique())
    }

    fn key_pressed(key: Key, modifiers: keyboard::Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
            key,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    }

    #[test]
    fn key_press_reports_raw_keyboard_boundary_message() {
        let event = key_pressed(Key::Character("l".into()), keyboard::Modifiers::CTRL);

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::KeyboardKeyPressed {
                key: Key::Character(value),
                modifiers,
                status: event::Status::Captured,
            }) if value == "l" && modifiers.control()
        ));
    }

    #[test]
    fn captured_left_release_finishes_tab_drag() {
        let event = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(message, Some(Message::TabDragFinished)));
    }

    #[test]
    fn captured_cursor_move_updates_cursor_position() {
        let position = iced::Point::new(12.0, 24.0);
        let event = Event::Mouse(mouse::Event::CursorMoved { position });

        let message = route_event(event, event::Status::Captured);

        let Some(Message::CursorMoved(received)) = message else {
            panic!("expected cursor movement message");
        };
        assert_eq!(received.x, position.x);
        assert_eq!(received.y, position.y);
    }

    #[test]
    fn captured_wheel_scroll_reports_scrollbar_activity() {
        let delta = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
        let event = Event::Mouse(mouse::Event::WheelScrolled { delta });

        let message = route_event(event, event::Status::Captured);

        assert!(matches!(
            message,
            Some(Message::CapturedWheelScrolled(received)) if received == delta
        ));
    }

    #[test]
    fn ignored_mouse_navigation_buttons_route_history_navigation() {
        assert!(matches!(
            route_event(
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)),
                event::Status::Ignored,
            ),
            Some(Message::Back)
        ));
        assert!(matches!(
            route_event(
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)),
                event::Status::Ignored,
            ),
            Some(Message::Forward)
        ));
    }

    #[test]
    fn modifier_change_updates_keyboard_modifier_state() {
        let event = Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::ALT));

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(
            message,
            Some(Message::KeyboardModifiersChanged(modifiers)) if modifiers.alt()
        ));
    }

    #[test]
    fn named_key_press_keeps_event_status_for_stateful_router() {
        let event = key_pressed(
            Key::Named(key::Named::ArrowDown),
            keyboard::Modifiers::default(),
        );

        let message = route_event(event, event::Status::Ignored);

        assert!(matches!(
            message,
            Some(Message::KeyboardKeyPressed {
                key: Key::Named(key::Named::ArrowDown),
                status: event::Status::Ignored,
                ..
            })
        ));
    }
}
