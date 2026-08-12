use iced::mouse;
use iced::widget::{button, column, container, mouse_area, row, tooltip, Column};
use iced::{Alignment, Background, Border, Element, Length, Theme};

use crate::app::FileBrowser;
use crate::appearance::{
    button_hover_surface_color, context_menu_button_style, context_menu_style, subtle_border_color,
};
use crate::icons::IconSymbol;
use crate::model::{
    Message, WindowChromeLayout, WindowControlKind, WindowControlPlacement, WindowControlSide,
};
use crate::typography::readable_text;

use super::option_controls::{secondary_action_button, segmented_choice_row, SegmentedChoice};
use super::settings_group::{info_setting_row, muted_setting_text};
use super::toggle_switch::switch_control;
use super::{themed_icon, IconTone};

const WINDOW_CONTROL_ROW_HEIGHT: f32 = 46.0;
const WINDOW_CONTROL_SIDE_WIDTH: f32 = 150.0;
const WINDOW_CONTROL_VISIBILITY_WIDTH: f32 = 126.0;
const WINDOW_CONTROL_GRIP_SIZE: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowControlVisibilityControl {
    Toggle { visible: bool },
    AlwaysVisible,
}

pub(super) fn window_control_settings_row(browser: &FileBrowser) -> Element<'_, Message> {
    info_setting_row(
        column![
            muted_setting_text("Window layout", 12),
            window_layout_selector(browser),
            side_control_group(browser, WindowControlSide::Left),
            side_control_group(browser, WindowControlSide::Right),
            secondary_action_button("Restore defaults", Message::WindowControlsReset),
        ]
        .spacing(8)
        .width(Length::Fill)
        .into(),
    )
}

fn window_layout_selector(browser: &FileBrowser) -> Element<'static, Message> {
    let selected = browser.user_config().window_controls.layout();
    segmented_choice_row(
        WindowChromeLayout::ALL
            .into_iter()
            .map(|layout| SegmentedChoice {
                label: layout.label(),
                selected: layout == selected,
                message: Message::WindowChromeLayoutSelected(layout),
            })
            .collect(),
    )
}

fn side_control_group(browser: &FileBrowser, side: WindowControlSide) -> Element<'_, Message> {
    let placements = browser
        .user_config()
        .window_controls
        .placements_on(side)
        .collect::<Vec<_>>();
    let mut controls = Column::new()
        .spacing(5)
        .width(Length::Fill)
        .push(muted_setting_text(
            match side {
                WindowControlSide::Left => "Left side",
                WindowControlSide::Right => "Right side",
            },
            12,
        ));

    if placements.is_empty() {
        controls = controls.push(
            container(readable_text("No controls").size(11))
                .padding([7, 10])
                .width(Length::Fill),
        );
    } else {
        for placement in placements {
            controls = controls.push(window_control_row(browser, placement));
        }
    }
    controls.into()
}

fn window_control_row(
    browser: &FileBrowser,
    placement: WindowControlPlacement,
) -> Element<'static, Message> {
    let kind = placement.kind();
    let dragged = browser.dragged_window_control() == Some(kind);
    let drop_target = browser.window_control_reorder_target() == Some(kind);
    let content = row![
        reorder_handle(kind),
        readable_text(kind.label()).size(12).width(Length::Fill),
        visibility_control(placement),
        side_selector(placement),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let row = container(content)
        .padding([6, 8])
        .width(Length::Fill)
        .height(Length::Fixed(WINDOW_CONTROL_ROW_HEIGHT))
        .style(move |theme| window_control_row_style(theme, dragged, drop_target));

    mouse_area(row)
        .on_enter(Message::WindowControlReorderTargetEntered(kind))
        .on_exit(Message::WindowControlReorderTargetExited(kind))
        .on_release(Message::WindowControlReorderFinished)
        .into()
}

fn reorder_handle(kind: WindowControlKind) -> Element<'static, Message> {
    let grip = container(themed_icon(
        IconSymbol::GripVertical,
        IconTone::Normal,
        WINDOW_CONTROL_GRIP_SIZE,
    ))
    .padding(7)
    .center_x(Length::Shrink)
    .center_y(Length::Shrink);
    let grip = mouse_area(grip)
        .on_press(Message::WindowControlReorderStarted(kind))
        .interaction(mouse::Interaction::Grab);

    tooltip(
        grip,
        container(readable_text("Drag to reorder").size(11))
            .padding([5, 7])
            .style(context_menu_style),
        tooltip::Position::Bottom,
    )
    .into()
}

fn visibility_control(placement: WindowControlPlacement) -> Element<'static, Message> {
    match window_control_visibility_control(placement) {
        WindowControlVisibilityControl::AlwaysVisible => {
            container(readable_text("Always visible").size(11))
                .width(Length::Fixed(WINDOW_CONTROL_VISIBILITY_WIDTH))
                .center_x(Length::Fill)
                .into()
        }
        WindowControlVisibilityControl::Toggle { visible } => {
            let content = row![
                readable_text("Show").size(11).width(Length::Fill),
                switch_control(visible),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            button(container(content).padding([3, 6]).width(Length::Fill))
                .on_press(Message::WindowControlVisibilityToggled(placement.kind()))
                .width(Length::Fixed(WINDOW_CONTROL_VISIBILITY_WIDTH))
                .style(context_menu_button_style())
                .into()
        }
    }
}

fn window_control_visibility_control(
    placement: WindowControlPlacement,
) -> WindowControlVisibilityControl {
    if placement.kind() == WindowControlKind::Close {
        WindowControlVisibilityControl::AlwaysVisible
    } else {
        WindowControlVisibilityControl::Toggle {
            visible: placement.visibility().is_visible(),
        }
    }
}

fn side_selector(placement: WindowControlPlacement) -> Element<'static, Message> {
    container(segmented_choice_row(
        WindowControlSide::ALL
            .into_iter()
            .map(|side| SegmentedChoice {
                label: side.label(),
                selected: side == placement.side(),
                message: Message::WindowControlSideSelected(placement.kind(), side),
            })
            .collect(),
    ))
    .width(Length::Fixed(WINDOW_CONTROL_SIDE_WIDTH))
    .into()
}

fn window_control_row_style(theme: &Theme, dragged: bool, drop_target: bool) -> container::Style {
    container::Style {
        background: (dragged || drop_target)
            .then(|| Background::Color(button_hover_surface_color(theme))),
        border: Border {
            color: subtle_border_color(theme),
            width: if drop_target { 2.0 } else { 1.0 },
            radius: 6.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowControlsConfig;

    #[test]
    fn default_rows_use_two_toggles_and_an_always_visible_close_control() {
        let config = WindowControlsConfig::default();
        let presentations = config
            .placements()
            .iter()
            .copied()
            .map(window_control_visibility_control)
            .collect::<Vec<_>>();

        assert_eq!(
            presentations,
            vec![
                WindowControlVisibilityControl::Toggle { visible: true },
                WindowControlVisibilityControl::Toggle { visible: true },
                WindowControlVisibilityControl::AlwaysVisible,
            ]
        );
    }
}
