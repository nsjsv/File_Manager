use file_core::FileOperationVerification;
use iced::widget::{button, column, container, row, Button};
use iced::{Alignment, Element, Length};

use crate::appearance::{context_menu_button_style, selected_sidebar_item_style};
use crate::model::Message;
use crate::typography::readable_text;

use super::toggle_switch::switch_control;

pub(super) fn file_operation_verification_options(
    selected: FileOperationVerification,
) -> Element<'static, Message> {
    strong_verification_button(selected).into()
}

fn strong_verification_button(selected: FileOperationVerification) -> Button<'static, Message> {
    let is_strong = selected == FileOperationVerification::Strong;
    let next_verification = if is_strong {
        FileOperationVerification::BasicMetadata
    } else {
        FileOperationVerification::Strong
    };
    let option_text = column![
        row![
            readable_text("Strong Verification")
                .size(12)
                .width(Length::Fill),
            switch_control(is_strong),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        readable_text("Compare copied file content hashes after standard metadata checks.")
            .size(11)
            .width(Length::Fill),
    ]
    .spacing(3);
    let label = container(option_text).padding([5, 8]).width(Length::Fill);
    let label = if is_strong {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::FileOperationVerificationSelected(
            next_verification,
        ))
        .width(Length::Fill)
        .style(context_menu_button_style())
}
