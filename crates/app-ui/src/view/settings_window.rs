use std::fmt;

use desktop_linux::{TerminalEmulator, TERMINAL_EMULATOR_OPTIONS};
use iced::widget::{
    button, column, container, pick_list, row, stack, svg, text, text_input, Button, Column, Space,
    Svg,
};
use iced::{window, Alignment, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::context_menu_button_style;
use crate::config::{StartupLocationPolicy, UiLanguageSetting};
use crate::icons::{rotated_chevron_right_view, IconSymbol};
use crate::matugen_theme::{ColorSchemeFamily, ColorSchemePreset, ContrastWarnings, ThemeMode};
use crate::model::{
    Message, ScrollbarRegion, ScrollbarViewport, ScrollbarVisibility, SettingsCategory,
};
use crate::typography::{localized_text, readable_text};

use super::application_logs::application_logs_settings_detail;
use super::auxiliary_window_content_without_title;
use super::auxiliary_window_layout::{
    auxiliary_detail_scroller, auxiliary_detail_surface_with_sidebar_space,
    auxiliary_full_height_sidebar, auxiliary_sidebar_button,
};
use super::file_operation_verification_settings::file_operation_verification_options;
use super::network_settings::{network_thumbnails_row, preview_size_limit_rows};
use super::option_controls::{
    inactive_segmented_choice_row, segmented_choice_button_style, segmented_choice_row,
    SegmentedChoice,
};
use super::rendering_settings::rendering_gpu_preference_row;
use super::search_settings::search_settings_detail;
use super::settings_group::{
    info_setting_row, labeled_setting_row, settings_card, settings_group, toggle_setting_row,
    SETTINGS_GROUP_SPACING,
};
use super::shortcut_settings::shortcut_settings_section;
use super::window_control_settings::window_control_settings_row;
use super::IconTone;

const SETTINGS_DROPDOWN_WIDTH: f32 = 220.0;

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

pub(crate) fn view_settings_window(
    browser: &FileBrowser,
    window: window::Id,
) -> Element<'_, Message> {
    let sidebar = settings_category_sidebar(browser.selected_settings_category);
    let detail = auxiliary_detail_surface_with_sidebar_space(settings_category_detail(browser));
    let frame_state = browser.window_frame_state(window);
    let window_content = auxiliary_window_content_without_title(
        detail,
        &browser.user_config().window_controls,
        window,
        frame_state,
    );
    let content = stack![window_content, sidebar]
        .width(Length::Fill)
        .height(Length::Fill);
    super::window_resize_frame(content.into(), window, frame_state)
}

fn settings_category_sidebar(selected: SettingsCategory) -> Element<'static, Message> {
    let mut categories = Column::new()
        .push(readable_text("Settings").size(16))
        .spacing(4);
    for category in SettingsCategory::ALL {
        categories = categories.push(settings_category_button(category, selected));
    }
    auxiliary_full_height_sidebar(categories)
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
    let scrollbar_region = ScrollbarRegion::Settings;
    let scrollbar_visibility = browser.scrollbar_visibility_for(&scrollbar_region);
    let scrollbar_viewport = browser.scrollbar_viewport_for(&scrollbar_region);
    match browser.selected_settings_category {
        SettingsCategory::General => {
            general_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
        SettingsCategory::Appearance => {
            appearance_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
        SettingsCategory::Files => {
            files_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
        SettingsCategory::Search => {
            search_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
        SettingsCategory::Shortcuts => {
            shortcut_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
        SettingsCategory::Logs => {
            application_logs_settings_detail(browser, scrollbar_visibility, scrollbar_viewport)
        }
    }
}

fn general_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
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
        scrollbar_viewport,
    )
}

fn appearance_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![
            settings_group(
                "Theme",
                vec![
                    labeled_setting_row("Mode", theme_mode_selector(browser)),
                    color_scheme_setting_row(browser),
                    custom_color_scheme_import_row(browser),
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
        scrollbar_viewport,
    )
}

fn files_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
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
                    preview_size_limit_rows(browser),
                ],
            ),
        ]
        .spacing(SETTINGS_GROUP_SPACING)
        .width(Length::Fill),
        scrollbar_visibility,
        scrollbar_viewport,
    )
}

fn shortcut_settings_detail(
    browser: &FileBrowser,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'_, Message> {
    settings_detail_scroller(
        column![shortcut_settings_section(browser)]
            .spacing(SETTINGS_GROUP_SPACING)
            .width(Length::Fill),
        scrollbar_visibility,
        scrollbar_viewport,
    )
}

fn settings_detail_scroller<'a>(
    content: Column<'a, Message>,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_viewport: Option<ScrollbarViewport>,
) -> Element<'a, Message> {
    auxiliary_detail_scroller(
        content,
        ScrollbarRegion::Settings,
        scrollbar_visibility,
        scrollbar_viewport,
        Message::SettingsScrolled,
    )
}

fn theme_mode_selector(browser: &FileBrowser) -> Element<'static, Message> {
    let selected = browser.user_config().theme_mode;
    let choices = ThemeMode::ALL
        .into_iter()
        .map(|mode| SegmentedChoice {
            label: mode.label(),
            selected: mode == selected,
            message: Message::ThemeModeSelected(mode),
        })
        .collect();

    if browser.user_config().color_scheme == ColorSchemePreset::Matugen {
        inactive_segmented_choice_row(choices)
    } else {
        segmented_choice_row(choices)
    }
}

fn color_scheme_setting_row(browser: &FileBrowser) -> Element<'_, Message> {
    container(
        column![
            readable_text("Color scheme").size(12),
            color_scheme_grid(browser),
        ]
        .spacing(8),
    )
    .padding([8, 12])
    .width(Length::Fill)
    .into()
}

fn custom_color_scheme_import_row(browser: &FileBrowser) -> Element<'_, Message> {
    let import = button(
        row![
            super::themed_icon(IconSymbol::Download, IconTone::Normal, 14.0),
            readable_text("Import JSON").size(12),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .on_press(Message::CustomColorSchemeImportPressed)
    .padding([6, 10])
    .style(context_menu_button_style());
    let mut content = column![row![
        readable_text("Custom color scheme")
            .size(12)
            .width(Length::Fill),
        import,
    ]
    .spacing(8)
    .align_y(Alignment::Center)]
    .spacing(5);

    if browser.user_config().color_scheme == ColorSchemePreset::Custom {
        let warnings = browser
            .user_config()
            .custom_color_scheme
            .contrast_warnings(browser.active_appearance_mode());
        if !warnings.is_empty() {
            content = content.push(custom_color_scheme_contrast_warning(warnings));
        }
    }
    if let Some(error) = &browser.custom_color_scheme_import_error {
        content = content.push(
            row![
                super::themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, 13.0),
                text(format!(
                    "{}: {error}",
                    crate::localization::translate_current("Import failed")
                ))
                .size(11),
            ]
            .spacing(5)
            .align_y(Alignment::Center),
        );
    }

    info_setting_row(content.into())
}

fn custom_color_scheme_contrast_warning(warnings: ContrastWarnings) -> Element<'static, Message> {
    let label = match (warnings.background_text, warnings.surface_muted_text) {
        (true, true) => "Custom colors have low text contrast",
        (true, false) => "Background and text contrast is below 2.4:1",
        (false, true) => "Surface and muted text contrast is below 2.4:1",
        (false, false) => return Space::new().into(),
    };
    row![
        super::themed_icon(IconSymbol::TriangleAlert, IconTone::Warning, 13.0),
        localized_text(label).size(11),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .into()
}

fn color_scheme_grid(browser: &FileBrowser) -> Element<'_, Message> {
    let mode = browser.active_appearance_mode();
    let selected = browser.user_config().color_scheme.effective_for_mode(mode);
    let expanded = browser.expanded_color_scheme_family;
    let mut grid = Column::new().spacing(8).width(Length::Fill);

    for families in ColorSchemeFamily::ALL.chunks(3) {
        let mut family_row = row![].spacing(8).width(Length::Fill);
        for family in families {
            family_row =
                family_row.push(color_scheme_family_card(browser, *family, mode, selected));
        }
        for _ in families.len()..3 {
            family_row = family_row.push(
                Space::new()
                    .width(Length::FillPortion(1))
                    .height(Length::Fixed(112.0)),
            );
        }
        grid = grid.push(family_row);

        if let Some(family) = expanded.filter(|family| families.contains(family)) {
            grid = grid.push(color_scheme_style_row(browser, family, mode, selected));
        }
    }

    grid.into()
}

fn color_scheme_family_card(
    browser: &FileBrowser,
    family: ColorSchemeFamily,
    mode: crate::matugen_theme::AppearanceMode,
    selected: ColorSchemePreset,
) -> Button<'static, Message> {
    let preview_preset = if selected.family() == family {
        browser.user_config().color_scheme
    } else {
        family.default_preset()
    };
    let is_selected = selected.family() == family;
    let has_styles = family.styles(mode).len() > 1;
    let check: Element<'static, Message> = if is_selected {
        super::themed_icon(IconSymbol::Check, IconTone::Selected, 13.0).into()
    } else {
        Space::new()
            .width(Length::Fixed(13.0))
            .height(Length::Fixed(13.0))
            .into()
    };
    let disclosure: Element<'static, Message> = if has_styles {
        rotated_chevron_right_view(
            if browser.expanded_color_scheme_family == Some(family) {
                90.0
            } else {
                0.0
            },
            13.0,
        )
        .style(super::icon_tone_style(IconTone::Normal))
        .into()
    } else {
        Space::new()
            .width(Length::Fixed(13.0))
            .height(Length::Fixed(13.0))
            .into()
    };
    let indicators = row![check, disclosure]
        .spacing(5)
        .height(Length::Fixed(14.0))
        .align_y(Alignment::Center);

    button(
        column![
            theme_preview(browser.theme_preview_colors(preview_preset), 48.0),
            container(localized_text(family.label()).size(11))
                .width(Length::Fill)
                .center_x(Length::Fill),
            indicators,
        ]
        .spacing(6)
        .align_x(Alignment::Center),
    )
    .on_press(Message::ColorSchemeFamilySelected(family))
    .width(Length::FillPortion(1))
    .height(Length::Fixed(112.0))
    .padding(8)
    .style(segmented_choice_button_style(is_selected))
}

fn color_scheme_style_row(
    browser: &FileBrowser,
    family: ColorSchemeFamily,
    mode: crate::matugen_theme::AppearanceMode,
    selected: ColorSchemePreset,
) -> Element<'static, Message> {
    let mut styles = row![].spacing(6).width(Length::Fill);
    for preset in family.styles(mode) {
        let is_selected = selected == *preset;
        let check: Element<'static, Message> = if is_selected {
            super::themed_icon(IconSymbol::Check, IconTone::Selected, 12.0).into()
        } else {
            Space::new()
                .width(Length::Fixed(12.0))
                .height(Length::Fixed(12.0))
                .into()
        };
        styles = styles.push(
            button(
                column![
                    theme_preview(browser.theme_preview_colors(*preset), 28.0),
                    row![
                        container(localized_text(preset.style_label(mode)).size(10))
                            .width(Length::Fill)
                            .center_x(Length::Fill),
                        check,
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .on_press(Message::ColorSchemePresetSelected(*preset))
            .width(Length::FillPortion(1))
            .height(Length::Fixed(62.0))
            .padding(5)
            .style(segmented_choice_button_style(is_selected)),
        );
    }

    styles.into()
}

fn theme_preview(colors: [iced::Color; 3], size: f32) -> Svg<'static, Theme> {
    Svg::new(svg::Handle::from_memory(theme_preview_svg(colors)))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
}

fn theme_preview_svg(colors: [iced::Color; 3]) -> Vec<u8> {
    let [background, primary, text] = colors;
    let background = color_hex(background);
    let primary = color_hex(primary);
    let text = color_hex(text);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 48 48\"><defs><clipPath id=\"circle\"><circle cx=\"24\" cy=\"24\" r=\"22\"/></clipPath></defs><g clip-path=\"url(#circle)\"><path fill=\"{background}\" d=\"M0 0h48L0 48z\"/><path fill=\"{primary}\" d=\"M48 0v48H0z\"/><path d=\"M0 48 48 0\" fill=\"none\" stroke=\"{text}\" stroke-width=\"3\"/></g><circle cx=\"24\" cy=\"24\" r=\"22\" fill=\"none\" stroke=\"{text}\" stroke-width=\"3\"/></svg>"
    )
    .into_bytes()
}

fn color_hex(color: iced::Color) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.r),
        channel(color.g),
        channel(color.b)
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_preview_clips_both_fills_and_divider_to_the_circle() {
        let svg = String::from_utf8(theme_preview_svg([
            iced::Color::from_rgb8(1, 2, 3),
            iced::Color::from_rgb8(4, 5, 6),
            iced::Color::from_rgb8(7, 8, 9),
        ]))
        .expect("preview SVG is UTF-8");

        assert!(svg.contains("fill=\"#010203\""));
        assert!(svg.contains("fill=\"#040506\""));
        assert!(svg.contains("stroke=\"#070809\""));
        let divider = svg.find("M0 48 48 0").expect("diagonal divider");
        let clipped_group_end = svg.find("</g>").expect("clipped preview group");
        assert!(divider < clipped_group_end);
    }
}
