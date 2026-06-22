use std::fmt;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{button, column, container, pick_list, row, Button, Column, Space};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility, SettingsCategory};
use crate::typography::readable_text;

use super::auxiliary_window_layout::{
    auxiliary_detail_scroller, auxiliary_sidebar, auxiliary_sidebar_button, auxiliary_split_window,
};
use super::file_operation_verification_settings::file_operation_verification_options;
use super::network_settings::network_settings_content;
use super::rendering_settings::rendering_gpu_preference_button;
use super::search_index_settings::search_index_settings_content;
use super::shortcut_settings::shortcut_settings_section;
use super::toggle_switch::switch_control;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalEmulatorPickOption(TerminalEmulator);

impl fmt::Display for TerminalEmulatorPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.label())
    }
}

pub(crate) fn view_settings_window(browser: &FileBrowser) -> Element<'_, Message> {
    let categories = settings_category_sidebar(browser.selected_settings_category);
    let detail = settings_category_detail(browser);

    auxiliary_split_window(categories, detail)
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new()
        .spacing(6)
        .push(readable_text("Settings").size(18))
        .push(Space::new().height(Length::Fixed(6.0)));

    for category in SettingsCategory::ALL {
        categories = categories.push(settings_category_button(category, selected));
    }

    auxiliary_sidebar(categories)
}

fn settings_category_button(
    category: SettingsCategory,
    selected: SettingsCategory,
) -> Button<'static, Message> {
    auxiliary_sidebar_button(
        category.label(),
        category == selected,
        Message::SettingsCategorySelected(category),
    )
}

fn settings_category_detail(browser: &FileBrowser) -> Element<'_, Message> {
    let scrollbar_visibility = browser.scrollbar_visibility_for(&ScrollbarRegion::Settings);
    match browser.selected_settings_category {
        SettingsCategory::General => general_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Network => network_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::SearchIndex => {
            search_index_settings_detail(browser, scrollbar_visibility)
        }
        SettingsCategory::FileOperations => {
            file_operation_settings_detail(browser, scrollbar_visibility)
        }
        SettingsCategory::Rendering => rendering_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Shortcuts => shortcut_settings_detail(browser, scrollbar_visibility),
    }
}

fn general_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
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
        scrollbar_visibility,
    )
}

fn network_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(network_settings_content(browser), scrollbar_visibility)
}

fn search_index_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(search_index_settings_content(browser), scrollbar_visibility)
}

fn file_operation_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("File Operations").size(20),
            readable_text("Verification").size(13),
            file_operation_verification_options(browser.file_operation_verification()),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn rendering_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Rendering").size(20),
            rendering_gpu_preference_button(browser.rendering_gpu_preference),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn shortcut_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            readable_text("Shortcuts").size(20),
            shortcut_settings_section(browser),
        ]
        .spacing(10)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn settings_detail_scroller<'a>(
    content: Column<'a, Message>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    auxiliary_detail_scroller(content, scrollbar_visibility, Message::SettingsScrolled)
}

fn terminal_emulator_options(selected: TerminalEmulator) -> Element<'static, Message> {
    let options = TERMINAL_EMULATOR_OPTIONS
        .iter()
        .copied()
        .map(TerminalEmulatorPickOption)
        .collect::<Vec<_>>();

    pick_list(
        options,
        Some(TerminalEmulatorPickOption(selected)),
        |selected| Message::TerminalEmulatorSelected(selected.0),
    )
    .width(Length::Fill)
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn hidden_files_visibility_button(browser: &FileBrowser) -> Button<'static, Message> {
    let label = row![
        readable_text("Show Hidden Files")
            .size(12)
            .width(Length::Fill),
        switch_control(browser.options.include_hidden),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    button(container(label).padding([5, 8]).width(Length::Fill))
        .on_press(Message::ShowHiddenFilesToggled)
        .width(Length::Fill)
        .style(context_menu_button_style())
}
