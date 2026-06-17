use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{button, column, container, row, scrollable, Button, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::{
    app_content_style, auto_hide_scrollbar_style, auto_hide_vertical_scrollbar_direction,
    context_menu_button_style, context_menu_style, selected_sidebar_item_style,
};
use crate::model::{Message, ScrollbarVisibility, SettingsCategory};
use crate::typography::readable_text;

use super::file_operation_verification_settings::file_operation_verification_options;
use super::rendering_settings::rendering_gpu_preference_button;
use super::search_index_settings::search_index_settings_content;
use super::shortcut_settings::shortcut_settings_section;
use super::toggle_switch::switch_control;

const SETTINGS_CATEGORY_WIDTH: f32 = 196.0;

pub(crate) fn view_settings_window(browser: &FileBrowser) -> Element<'_, Message> {
    let categories = settings_category_sidebar(browser.selected_settings_category);
    let detail = settings_category_detail(browser);

    container(row![categories, detail].height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(app_content_style)
        .into()
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new()
        .spacing(6)
        .padding(14)
        .push(readable_text("Settings").size(18))
        .push(Space::new().height(Length::Fixed(6.0)));

    for category in SettingsCategory::ALL {
        categories = categories.push(settings_category_button(category, selected));
    }

    container(categories)
        .width(Length::Fixed(SETTINGS_CATEGORY_WIDTH))
        .height(Length::Fill)
        .style(context_menu_style)
        .into()
}

fn settings_category_button(
    category: SettingsCategory,
    selected: SettingsCategory,
) -> Button<'static, Message> {
    let label = container(readable_text(category.label()).size(13))
        .padding([7, 10])
        .width(Length::Fill);
    let label = if category == selected {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::SettingsCategorySelected(category))
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn settings_category_detail(browser: &FileBrowser) -> Element<'_, Message> {
    match browser.selected_settings_category {
        SettingsCategory::General => general_settings_detail(browser),
        SettingsCategory::SearchIndex => search_index_settings_detail(browser),
        SettingsCategory::FileOperations => file_operation_settings_detail(browser),
        SettingsCategory::Rendering => rendering_settings_detail(browser),
        SettingsCategory::Shortcuts => shortcut_settings_detail(browser),
    }
}

fn general_settings_detail(browser: &FileBrowser) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("General").size(20),
            readable_text("File display").size(13),
            hidden_files_visibility_button(browser),
            readable_text("Terminal").size(13),
            terminal_emulator_options(browser.terminal_emulator),
        ]
        .spacing(10)
        .width(Length::Fill),
        browser.scrollbar_visibility,
    )
}

fn search_index_settings_detail(browser: &FileBrowser) -> Element<'_, Message> {
    settings_detail_scroller(
        search_index_settings_content(browser),
        browser.scrollbar_visibility,
    )
}

fn file_operation_settings_detail(browser: &FileBrowser) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("File Operations").size(20),
            readable_text("Verification").size(13),
            file_operation_verification_options(browser.file_operation_verification()),
        ]
        .spacing(10)
        .width(Length::Fill),
        browser.scrollbar_visibility,
    )
}

fn rendering_settings_detail(browser: &FileBrowser) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Rendering").size(20),
            rendering_gpu_preference_button(browser.rendering_gpu_preference),
        ]
        .spacing(10)
        .width(Length::Fill),
        browser.scrollbar_visibility,
    )
}

fn shortcut_settings_detail(browser: &FileBrowser) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Shortcuts").size(20),
            shortcut_settings_section(browser),
        ]
        .spacing(10)
        .width(Length::Fill),
        browser.scrollbar_visibility,
    )
}

fn settings_detail_scroller<'a>(
    content: Column<'a, Message>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    let content = container(content).padding(18).width(Length::Fill);
    let scroller = scrollable(content)
        .direction(auto_hide_vertical_scrollbar_direction(
            scrollbar_visibility,
            6.0,
        ))
        .style(auto_hide_scrollbar_style(scrollbar_visibility));

    container(scroller)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn terminal_emulator_options(selected: TerminalEmulator) -> Element<'static, Message> {
    let mut options = Column::new().spacing(4);
    for terminal_emulator in TERMINAL_EMULATOR_OPTIONS {
        options = options.push(terminal_emulator_button(*terminal_emulator, selected));
    }
    options.into()
}

fn hidden_files_visibility_button(browser: &FileBrowser) -> Button<'static, Message> {
    let status = if browser.options.include_hidden {
        "On"
    } else {
        "Off"
    };
    let label = row![
        readable_text("Show Hidden Files")
            .size(12)
            .width(Length::Fill),
        readable_text(status).size(12),
        switch_control(browser.options.include_hidden),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ShowHiddenFilesToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}

fn terminal_emulator_button(
    terminal_emulator: TerminalEmulator,
    selected_emulator: TerminalEmulator,
) -> Button<'static, Message> {
    let label = container(readable_text(terminal_emulator.label()).size(12))
        .padding([5, 8])
        .width(Length::Fill);
    let label = if terminal_emulator == selected_emulator {
        label.style(selected_sidebar_item_style)
    } else {
        label
    };

    button(label)
        .on_press(Message::TerminalEmulatorSelected(terminal_emulator))
        .width(Length::Fill)
        .style(context_menu_button_style())
}
