use iced::advanced::widget as advanced_widget;
use iced::advanced::widget::operation::{Focusable, Operation, Outcome, TextInput};
use iced::{Rectangle, Task};

use crate::model::Message;

pub(super) fn select_focused_text_or_visible_files_command() -> Task<Message> {
    advanced_widget::operate(FocusedTextInputSelectAll::default())
}

#[derive(Default)]
struct FocusedTextInputSelectAll {
    focused_widget_id: Option<advanced_widget::Id>,
    selected_focused_text: bool,
}

impl Operation<Message> for FocusedTextInputSelectAll {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Message>)) {
        operate(self);
    }

    fn focusable(
        &mut self,
        id: Option<&advanced_widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn Focusable,
    ) {
        if state.is_focused() {
            self.focused_widget_id = id.cloned();
        }
    }

    fn text_input(
        &mut self,
        id: Option<&advanced_widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn TextInput,
    ) {
        if self.focused_widget_id.as_ref() == id {
            state.select_all();
            self.selected_focused_text = true;
        }
    }

    fn finish(&self) -> Outcome<Message> {
        if self.selected_focused_text {
            Outcome::None
        } else {
            Outcome::Some(Message::SelectAll)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TextInputProbe {
        select_all_calls: usize,
    }

    impl TextInput for TextInputProbe {
        fn text(&self) -> &str {
            ""
        }

        fn move_cursor_to_front(&mut self) {}

        fn move_cursor_to_end(&mut self) {}

        fn move_cursor_to(&mut self, _position: usize) {}

        fn select_all(&mut self) {
            self.select_all_calls += 1;
        }

        fn select_range(&mut self, _start: usize, _end: usize) {}
    }

    struct FocusProbe {
        focused: bool,
    }

    impl Focusable for FocusProbe {
        fn is_focused(&self) -> bool {
            self.focused
        }

        fn focus(&mut self) {
            self.focused = true;
        }

        fn unfocus(&mut self) {
            self.focused = false;
        }
    }

    #[test]
    fn focused_text_input_consumes_file_selection() {
        let mut operation = FocusedTextInputSelectAll::default();
        let input_id: advanced_widget::Id = iced::widget::Id::new("focused-input").into();
        let mut focus_probe = FocusProbe { focused: true };
        let mut text_probe = TextInputProbe::default();

        operation.focusable(Some(&input_id), Rectangle::default(), &mut focus_probe);
        operation.text_input(Some(&input_id), Rectangle::default(), &mut text_probe);

        assert_eq!(text_probe.select_all_calls, 1);
        assert!(matches!(operation.finish(), Outcome::None));
    }

    #[test]
    fn missing_focused_text_input_falls_back_to_file_selection() {
        let mut operation = FocusedTextInputSelectAll::default();
        let input_id: advanced_widget::Id = iced::widget::Id::new("unfocused-input").into();
        let mut focus_probe = FocusProbe { focused: false };
        let mut text_probe = TextInputProbe::default();

        operation.focusable(Some(&input_id), Rectangle::default(), &mut focus_probe);
        operation.text_input(Some(&input_id), Rectangle::default(), &mut text_probe);

        assert_eq!(text_probe.select_all_calls, 0);
        assert!(matches!(
            operation.finish(),
            Outcome::Some(Message::SelectAll)
        ));
    }
}
