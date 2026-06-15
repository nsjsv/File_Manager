use file_core::FileOperationVerification;
use iced::widget::{button, container, Button, Column};
use iced::{Element, Length};

use crate::appearance::{context_menu_button_style, selected_sidebar_item_style};
use crate::config::{file_operation_verification_description, file_operation_verification_label};
use crate::model::Message;
use crate::typography::readable_text;

const FILE_OPERATION_VERIFICATION_OPTIONS: [FileOperationVerification; 2] = [
    FileOperationVerification::BasicMetadata,
    FileOperationVerification::Strong,
];

pub(super) fn file_operation_verification_options(
    selected: FileOperationVerification,
) -> Element<'static, Message> {
    let mut options = iced::widget::Column::new().spacing(4);
    for verification in FILE_OPERATION_VERIFICATION_OPTIONS {
        options = options.push(file_operation_verification_button(verification, selected));
    }
    options.into()
}

fn file_operation_verification_button(
    verification: FileOperationVerification,
    selected: FileOperationVerification,
) -> Button<'static, Message> {
    let option_text = Column::new()
        .spacing(2)
        .push(readable_text(file_operation_verification_label(verification)).size(12))
        .push(
            readable_text(file_operation_verification_description(verification))
                .size(11)
                .width(Length::Fill),
        );
    let label = container(option_text).padding([5, 8]).width(Length::Fill);
    let label = if verification == selected {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::FileOperationVerificationSelected(verification))
        .width(Length::Fill)
        .style(context_menu_button_style())
}
