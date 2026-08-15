use std::fmt;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{button, column, container, pick_list, row, text_input, Button, Column};
use iced::{Alignment, Element, Length};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::config::{StartupLocationPolicy, UiLanguageSetting};
use crate::matugen_theme::{ColorSchemePreset, ThemeMode};
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility, SettingsCategory};
use crate::typography::{localized_text, readable_text};

use super::application_logs::application_logs_settings_detail;
use super::auxiliary_window_layout::{
    auxiliary_detail_scroller, auxiliary_sidebar, auxiliary_sidebar_button, auxiliary_split_window,
};
use super::file_operation_verification_settings::file_operation_verification_options;
use super::network_settings::{max_preview_file_size_row, network_thumbnails_row};
use super::rendering_settings::rendering_gpu_preference_row;
use super::search_settings::search_settings_detail;
use super::settings_group::{
    info_setting_row, labeled_setting_row, settings_card, settings_group, toggle_setting_row,
    SETTINGS_GROUP_SPACING,
};
use super::shortcut_settings::shortcut_settings_section;
use super::window_control_settings::window_control_settings_row;

const SETTINGS_DROPDOWN_WIDTH: f32 = 220.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemeModePickOption(ThemeMode);

impl fmt::Display for ThemeModePickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColorSchemePickOption(ColorSchemePreset);

impl fmt::Display for ColorSchemePickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TerminalEmulatorPickOption(TerminalEmulator);

impl fmt::Display for TerminalEmulatorPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(self.0.label()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LanguageSettingPickOption(UiLanguageSetting);

impl fmt::Display for LanguageSettingPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(
            language_setting_label(self.0),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupLocationPickOption(StartupLocationPolicy);

impl fmt::Display for StartupLocationPickOption {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&crate::localization::translate_current(
            startup_location_label(self.0),
        ))
    }
}

pub(crate) fn view_settings_window(browser: &FileBrowser) -> Element<'_, Message> {
    let categories = settings_category_sidebar(browser.selected_settings_category);
    let detail = settings_category_detail(browser);

    auxiliary_split_window(categories, detail)
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new().spacing(4);
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
        SettingsCategory::Appearance => appearance_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Files => files_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Search => search_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Shortcuts => shortcut_settings_detail(browser, scrollbar_visibility),
        SettingsCategory::Logs => application_logs_settings_detail(browser, scrollbar_visibility),
    }
}

fn general_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    let mut rows = vec![
        labeled_setting_row("Language", language_setting_dropdown(browser)),
        labeled_setting_row("Startup location", startup_location_dropdown(browser)),
    ];
    if browser.user_config().startup_location_policy == StartupLocationPolicy::CustomDirectory {
        rows.push(startup_custom_directory_row(browser));
    }
    rows.push(labeled_setting_row(
        "Terminal",
        terminal_emulator_dropdown(browser.terminal_emulator),
    ));

    settings_detail_scroller(
        column![settings_card(rows)]
            .spacing(SETTINGS_GROUP_SPACING)
            .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn appearance_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            settings_group(
                "Theme",
                vec![
                    labeled_setting_row("Mode", theme_mode_dropdown(browser)),
                    labeled_setting_row("Color scheme", color_scheme_dropdown(browser)),
                ],
            ),
            settings_group(
                "Window controls",
                vec![window_control_settings_row(browser)]
            ),
            settings_group(
                "Rendering",
                vec![rendering_gpu_preference_row(
                    browser.rendering_gpu_preference
                )],
            ),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn files_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            settings_group(
                "File display",
                vec![
                    toggle_setting_row(
                        "Show Hidden Files",
                        None,
                        browser.options.include_hidden,
                        Message::ShowHiddenFilesToggled,
                    ),
                    toggle_setting_row(
                        "Show Recursive Folder Size In List View",
                        None,
                        browser
                            .user_config()
                            .list_directory_size_display_mode
                            .uses_recursive_total_size(),
                        Message::ListDirectorySizeDisplayModeToggled,
                    ),
                ],
            ),
            settings_group(
                "Verification",
                vec![file_operation_verification_options(
                    browser.file_operation_verification(),
                )],
            ),
            settings_group(
                "Network",
                vec![
                    network_thumbnails_row(browser),
                    max_preview_file_size_row(browser),
                ],
            ),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn shortcut_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![shortcut_settings_section(browser)]
            .spacing(SETTINGS_GROUP_SPACING)
            .width(Length::Fill),
        scrollbar_visibility,
    )
}

fn settings_detail_scroller<'a>(
    content: Column<'a, Message>,
    scrollbar_visibility: ScrollbarVisibility,
) -> Element<'a, Message> {
    auxiliary_detail_scroller(
        content,
        ScrollbarRegion::Settings,
        scrollbar_visibility,
        Message::SettingsScrolled,
    )
}

fn theme_mode_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    let selected = ThemeModePickOption(browser.user_config().theme_mode);
    if browser.user_config().color_scheme == ColorSchemePreset::Matugen {
        return button(readable_text(selected.to_string()).size(12))
            .padding([5, 8])
            .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
            .style(context_menu_button_style())
            .into();
    }

    pick_list(
        ThemeMode::ALL.map(ThemeModePickOption),
        Some(selected),
        |selected| Message::ThemeModeSelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn color_scheme_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    pick_list(
        ColorSchemePreset::ALL.map(ColorSchemePickOption),
        Some(ColorSchemePickOption(browser.user_config().color_scheme)),
        |selected| Message::ColorSchemePresetSelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn language_setting_label(setting: UiLanguageSetting) -> &'static str {
    match setting {
        UiLanguageSetting::System => "Auto",
        UiLanguageSetting::English => "English",
        UiLanguageSetting::Chinese => "中文",
    }
}

fn startup_location_label(policy: StartupLocationPolicy) -> &'static str {
    match policy {
        StartupLocationPolicy::Home => "Home directory",
        StartupLocationPolicy::CustomDirectory => "Custom directory",
        StartupLocationPolicy::PreviousSession => "Previous state",
    }
}

fn language_setting_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    let options = [
        UiLanguageSetting::System,
        UiLanguageSetting::English,
        UiLanguageSetting::Chinese,
    ]
    .into_iter()
    .map(LanguageSettingPickOption)
    .collect::<Vec<_>>();

    pick_list(
        options,
        Some(LanguageSettingPickOption(
            browser.user_config().language_setting,
        )),
        |selected| Message::LanguageSettingSelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn startup_location_dropdown(browser: &FileBrowser) -> Element<'static, Message> {
    let options = [
        StartupLocationPolicy::Home,
        StartupLocationPolicy::CustomDirectory,
        StartupLocationPolicy::PreviousSession,
    ]
    .into_iter()
    .map(StartupLocationPickOption)
    .collect::<Vec<_>>();

    pick_list(
        options,
        Some(StartupLocationPickOption(
            browser.user_config().startup_location_policy,
        )),
        |selected| Message::StartupLocationPolicySelected(selected.0),
    )
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn terminal_emulator_dropdown(selected: TerminalEmulator) -> Element<'static, Message> {
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
    .width(Length::Fixed(SETTINGS_DROPDOWN_WIDTH))
    .text_size(12)
    .padding([5, 8])
    .into()
}

fn startup_custom_directory_row(browser: &FileBrowser) -> Element<'_, Message> {
    let input = text_input(
        &crate::localization::translate_current("Directory"),
        &browser.startup_custom_directory_input,
    )
    .on_input(Message::StartupCustomDirectoryInputChanged)
    .on_submit(Message::StartupCustomDirectoryCommitted)
    .padding([6, 8])
    .size(12)
    .width(Length::Fill);
    let save = button(container(readable_text("Save").size(12)).padding([6, 10]))
        .on_press(Message::StartupCustomDirectoryCommitted)
        .style(context_menu_button_style());
    let mut content = column![row![
        readable_text("Custom Startup Directory")
            .size(12)
            .width(Length::FillPortion(2)),
        input,
        save,
    ]
    .spacing(8)
    .align_y(Alignment::Center)]
    .spacing(3);
    if let Some(error) = &browser.startup_custom_directory_error {
        content = content.push(localized_text(error).size(11).width(Length::Fill));
    }
    info_setting_row(content.into())
}
