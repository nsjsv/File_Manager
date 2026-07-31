use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestMessage {
    Dismiss,
}

fn dismissal(policy: OutsideDismissalPolicy) -> OutsideClickDismissal<TestMessage> {
    OutsideClickDismissal {
        message: TestMessage::Dismiss,
        policy,
    }
}

#[test]
fn modal_inside_floating_bounds_stops_background_without_dismissal() {
    let decision = decide_floating_input::<TestMessage>(
        BackgroundInputPolicy::Blocked,
        None,
        FloatingPointerTarget::FloatingBounds,
        FloatingInputEvent::PrimaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: None,
            background_update: BackgroundUpdateDecision::Stop
        }
    );
}

#[test]
fn context_menu_inside_right_click_stops_background_without_dismissal() {
    let dismissal = dismissal(OutsideDismissalPolicy::ContextMenuReplacement);

    let decision = decide_floating_input(
        BackgroundInputPolicy::Blocked,
        Some(&dismissal),
        FloatingPointerTarget::FloatingBounds,
        FloatingInputEvent::SecondaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: None,
            background_update: BackgroundUpdateDecision::Stop
        }
    );
}

#[test]
fn modal_outside_left_click_captures_without_dismissal() {
    let decision = decide_floating_input::<TestMessage>(
        BackgroundInputPolicy::Blocked,
        None,
        FloatingPointerTarget::Background,
        FloatingInputEvent::PrimaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: None,
            background_update: BackgroundUpdateDecision::Capture
        }
    );
}

#[test]
fn blocking_dismissible_outside_left_click_dismisses_and_captures() {
    let dismissal = dismissal(OutsideDismissalPolicy::CapturedPrimaryPress);

    let decision = decide_floating_input(
        BackgroundInputPolicy::Blocked,
        Some(&dismissal),
        FloatingPointerTarget::Background,
        FloatingInputEvent::PrimaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: Some(TestMessage::Dismiss),
            background_update: BackgroundUpdateDecision::Capture
        }
    );
}

#[test]
fn blocking_dismissible_outside_right_click_stays_open_and_captures() {
    let dismissal = dismissal(OutsideDismissalPolicy::CapturedPrimaryPress);

    let decision = decide_floating_input(
        BackgroundInputPolicy::Blocked,
        Some(&dismissal),
        FloatingPointerTarget::Background,
        FloatingInputEvent::SecondaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: None,
            background_update: BackgroundUpdateDecision::Capture
        }
    );
}

#[test]
fn context_menu_outside_left_click_dismisses_and_captures() {
    let dismissal = dismissal(OutsideDismissalPolicy::ContextMenuReplacement);

    let decision = decide_floating_input(
        BackgroundInputPolicy::Blocked,
        Some(&dismissal),
        FloatingPointerTarget::Background,
        FloatingInputEvent::PrimaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: Some(TestMessage::Dismiss),
            background_update: BackgroundUpdateDecision::Capture
        }
    );
}

#[test]
fn context_menu_outside_right_click_dismisses_and_updates_background() {
    let dismissal = dismissal(OutsideDismissalPolicy::ContextMenuReplacement);

    let decision = decide_floating_input(
        BackgroundInputPolicy::Blocked,
        Some(&dismissal),
        FloatingPointerTarget::Background,
        FloatingInputEvent::SecondaryPress,
    );

    assert_eq!(
        decision,
        FloatingInputDecision {
            dismiss_message: Some(TestMessage::Dismiss),
            background_update: BackgroundUpdateDecision::Update
        }
    );
}

#[test]
fn floating_overlay_captures_mouse_events_inside_bounds() {
    let event = Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
    });
    let bounds = Rectangle::new(Point::new(10.0, 10.0), Size::new(100.0, 100.0));

    assert!(should_capture_floating_overlay_event(
        &event,
        mouse::Cursor::Available(Point::new(20.0, 20.0)),
        bounds,
    ));
    assert!(!should_capture_floating_overlay_event(
        &event,
        mouse::Cursor::Available(Point::new(200.0, 200.0)),
        bounds,
    ));
}
