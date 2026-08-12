use iced::alignment::{Horizontal, Vertical};
use iced::widget::{button, container, mouse_area, tooltip, Column, Row, Space, Stack};
use iced::{mouse, window, Element, Length};

use crate::appearance::{
    context_menu_style, window_close_button_style, window_control_button_style,
    window_title_bar_style, window_top_bar_style,
};
use crate::icons::IconSymbol;
use crate::model::{
    BrowserPaneId, BrowserPaneLayout, Message, SplitAxis, WindowChromeLayout, WindowControlKind,
    WindowControlSide, WindowControlsConfig, WindowFrameState, WINDOW_TITLE_BAR_HEIGHT,
    WINDOW_TOP_BAR_HEIGHT,
};
use crate::typography::{localized_text, readable_text};

use super::{themed_icon, window_drag_region::window_drag_region, IconTone};

const WINDOW_CONTROL_WIDTH: f32 = 36.0;
const WINDOW_CONTROL_HEIGHT: f32 = 32.0;
const WINDOW_CONTROL_ICON_SIZE: f32 = 12.0;
const WINDOW_CONTROL_VERTICAL_PADDING: f32 =
    (WINDOW_CONTROL_HEIGHT - WINDOW_CONTROL_ICON_SIZE) / 2.0;
const WINDOW_CONTROL_HORIZONTAL_PADDING: f32 =
    (WINDOW_CONTROL_WIDTH - WINDOW_CONTROL_ICON_SIZE) / 2.0;
const WINDOW_CONTROL_SPACING: f32 = 2.0;
const WINDOW_TITLE_SIDE_RESERVE: u16 = 116;
const WINDOW_RESIZE_EDGE_WIDTH: f32 = 5.0;
const WINDOW_RESIZE_CORNER_WIDTH: f32 = 11.0;
const STACKED_PANE_NAVIGATION_MAX_WIDTH: f32 = 500.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneNavigationLayout {
    SingleRow,
    StackedRows,
}

pub(crate) fn pane_navigation_layout(
    main_window_width: f32,
    sidebar_width: f32,
    pane_layout: BrowserPaneLayout,
) -> PaneNavigationLayout {
    let browser_width = (main_window_width - sidebar_width).max(1.0);
    let pane_width = match pane_layout {
        BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            ..
        } => browser_width / 2.0,
        BrowserPaneLayout::Single { .. }
        | BrowserPaneLayout::Split {
            axis: SplitAxis::Vertical,
            ..
        } => browser_width,
    };
    if pane_width < STACKED_PANE_NAVIGATION_MAX_WIDTH {
        PaneNavigationLayout::StackedRows
    } else {
        PaneNavigationLayout::SingleRow
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainPaneWindowChromeRole {
    Complete,
    LeftControls,
    RightControls,
    NoChrome,
}

impl MainPaneWindowChromeRole {
    pub(crate) fn shows_left_controls(self) -> bool {
        matches!(self, Self::Complete | Self::LeftControls)
    }

    pub(crate) fn shows_right_controls(self) -> bool {
        matches!(self, Self::Complete | Self::RightControls)
    }

    pub(crate) fn owns_window_drag_region(self) -> bool {
        self != Self::NoChrome
    }
}

pub(crate) fn main_pane_window_chrome_role(
    layout: BrowserPaneLayout,
    pane_id: BrowserPaneId,
) -> MainPaneWindowChromeRole {
    match layout {
        BrowserPaneLayout::Single { active } if pane_id == active => {
            MainPaneWindowChromeRole::Complete
        }
        BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            first,
            ..
        } if pane_id == first => MainPaneWindowChromeRole::LeftControls,
        BrowserPaneLayout::Split {
            axis: SplitAxis::Horizontal,
            second,
            ..
        } if pane_id == second => MainPaneWindowChromeRole::RightControls,
        BrowserPaneLayout::Split {
            axis: SplitAxis::Vertical,
            first,
            ..
        } if pane_id == first => MainPaneWindowChromeRole::Complete,
        BrowserPaneLayout::Single { .. } | BrowserPaneLayout::Split { .. } => {
            MainPaneWindowChromeRole::NoChrome
        }
    }
}

pub(crate) fn window_control_group(
    config: &WindowControlsConfig,
    side: WindowControlSide,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'static, Message> {
    let mut controls = Row::new()
        .spacing(WINDOW_CONTROL_SPACING)
        .height(Length::Fixed(WINDOW_CONTROL_HEIGHT));
    for placement in config
        .placements_on(side)
        .filter(|placement| placement.visibility().is_visible())
    {
        controls = controls.push(window_control_button(placement.kind(), window, frame_state));
    }
    controls.into()
}

fn window_control_button(
    kind: WindowControlKind,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'static, Message> {
    let (icon, label, message) = match kind {
        WindowControlKind::Minimize => (
            IconSymbol::Minus,
            "Minimize",
            Message::WindowMinimizeRequested(window),
        ),
        WindowControlKind::MaximizeRestore => match frame_state {
            WindowFrameState::Restored => (
                IconSymbol::Square,
                "Maximize",
                Message::WindowMaximizeToggled(window),
            ),
            WindowFrameState::Maximized => (
                IconSymbol::RestoreWindow,
                "Restore",
                Message::WindowMaximizeToggled(window),
            ),
        },
        WindowControlKind::Close => (
            IconSymbol::Close,
            "Close",
            Message::AuxiliaryWindowCloseRequested(window),
        ),
    };
    let style = if kind == WindowControlKind::Close {
        window_close_button_style
    } else {
        window_control_button_style
    };
    let control = button(themed_icon(
        icon,
        IconTone::Normal,
        WINDOW_CONTROL_ICON_SIZE,
    ))
    .on_press(message)
    .padding([
        WINDOW_CONTROL_VERTICAL_PADDING,
        WINDOW_CONTROL_HORIZONTAL_PADDING,
    ])
    .width(Length::Fixed(WINDOW_CONTROL_WIDTH))
    .height(Length::Fixed(WINDOW_CONTROL_HEIGHT))
    .style(style);

    tooltip(
        control,
        container(readable_text(label).size(11))
            .padding([5, 7])
            .style(context_menu_style),
        tooltip::Position::Bottom,
    )
    .into()
}

pub(crate) fn auxiliary_window_content<'a>(
    integrated_title: &'static str,
    separate_title: String,
    content: Element<'a, Message>,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'a, Message> {
    match config.layout() {
        WindowChromeLayout::IntegratedNavigation => {
            window_content_with_top_bar(integrated_title, content, config, window, frame_state)
        }
        WindowChromeLayout::SeparateTitleBar => separate_window_content_with_height(
            separate_title,
            content,
            config,
            window,
            frame_state,
            WINDOW_TOP_BAR_HEIGHT,
        ),
    }
}

fn window_content_with_top_bar<'a>(
    title: &'static str,
    content: Element<'a, Message>,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'a, Message> {
    Column::new()
        .spacing(0)
        .push(window_top_bar(title, config, window, frame_state))
        .push(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn window_top_bar(
    title: &'static str,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'static, Message> {
    let drag_surface = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(WINDOW_TOP_BAR_HEIGHT))
        .style(window_top_bar_style);
    let drag_surface = window_drag_region(drag_surface.into(), window);
    let content = Row::new()
        .spacing(10)
        .padding([8, 12])
        .align_y(iced::Alignment::Center)
        .push(window_control_group(
            config,
            WindowControlSide::Left,
            window,
            frame_state,
        ))
        .push(localized_text(title).size(16))
        .push(Space::new().width(Length::Fill))
        .push(window_control_group(
            config,
            WindowControlSide::Right,
            window,
            frame_state,
        ))
        .width(Length::Fill)
        .height(Length::Fixed(WINDOW_TOP_BAR_HEIGHT));

    Stack::with_children([drag_surface, content.into()])
        .width(Length::Fill)
        .height(Length::Fixed(WINDOW_TOP_BAR_HEIGHT))
        .into()
}

pub(crate) fn separate_window_content<'a>(
    title: String,
    content: Element<'a, Message>,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'a, Message> {
    separate_window_content_with_height(
        title,
        content,
        config,
        window,
        frame_state,
        WINDOW_TITLE_BAR_HEIGHT,
    )
}

fn separate_window_content_with_height<'a>(
    title: String,
    content: Element<'a, Message>,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
    height: f32,
) -> Element<'a, Message> {
    Column::new()
        .spacing(0)
        .push(separate_window_title_bar(
            title,
            config,
            window,
            frame_state,
            height,
        ))
        .push(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn separate_window_title_bar(
    title: String,
    config: &WindowControlsConfig,
    window: window::Id,
    frame_state: WindowFrameState,
    height: f32,
) -> Element<'static, Message> {
    let drag_surface = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .style(window_title_bar_style);
    let drag_surface = window_drag_region(drag_surface.into(), window);
    let title = container(localized_text(title).size(13))
        .padding([0, WINDOW_TITLE_SIDE_RESERVE])
        .center_x(Length::Fill)
        .center_y(Length::Fixed(height))
        .clip(true);
    let controls = Row::new()
        .spacing(0)
        .padding([4, 4])
        .align_y(iced::Alignment::Center)
        .push(window_control_group(
            config,
            WindowControlSide::Left,
            window,
            frame_state,
        ))
        .push(Space::new().width(Length::Fill))
        .push(window_control_group(
            config,
            WindowControlSide::Right,
            window,
            frame_state,
        ))
        .width(Length::Fill)
        .height(Length::Fixed(height));

    Stack::with_children([drag_surface, title.into(), controls.into()])
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
}

pub(crate) fn window_resize_frame<'a>(
    content: Element<'a, Message>,
    window: window::Id,
    frame_state: WindowFrameState,
) -> Element<'a, Message> {
    if frame_state == WindowFrameState::Maximized {
        return content;
    }

    let mut frame = Stack::with_children([content])
        .width(Length::Fill)
        .height(Length::Fill);
    for direction in [
        window::Direction::North,
        window::Direction::South,
        window::Direction::East,
        window::Direction::West,
        window::Direction::NorthEast,
        window::Direction::NorthWest,
        window::Direction::SouthEast,
        window::Direction::SouthWest,
    ] {
        frame = frame.push(window_resize_zone(window, direction));
    }
    frame.into()
}

fn window_resize_zone(
    window: window::Id,
    direction: window::Direction,
) -> Element<'static, Message> {
    let (width, height, horizontal, vertical, interaction) = match direction {
        window::Direction::North => (
            Length::Fill,
            Length::Fixed(WINDOW_RESIZE_EDGE_WIDTH),
            Horizontal::Center,
            Vertical::Top,
            mouse::Interaction::ResizingVertically,
        ),
        window::Direction::South => (
            Length::Fill,
            Length::Fixed(WINDOW_RESIZE_EDGE_WIDTH),
            Horizontal::Center,
            Vertical::Bottom,
            mouse::Interaction::ResizingVertically,
        ),
        window::Direction::East => (
            Length::Fixed(WINDOW_RESIZE_EDGE_WIDTH),
            Length::Fill,
            Horizontal::Right,
            Vertical::Center,
            mouse::Interaction::ResizingHorizontally,
        ),
        window::Direction::West => (
            Length::Fixed(WINDOW_RESIZE_EDGE_WIDTH),
            Length::Fill,
            Horizontal::Left,
            Vertical::Center,
            mouse::Interaction::ResizingHorizontally,
        ),
        window::Direction::NorthEast => (
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Horizontal::Right,
            Vertical::Top,
            mouse::Interaction::ResizingDiagonallyUp,
        ),
        window::Direction::NorthWest => (
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Horizontal::Left,
            Vertical::Top,
            mouse::Interaction::ResizingDiagonallyDown,
        ),
        window::Direction::SouthEast => (
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Horizontal::Right,
            Vertical::Bottom,
            mouse::Interaction::ResizingDiagonallyDown,
        ),
        window::Direction::SouthWest => (
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Length::Fixed(WINDOW_RESIZE_CORNER_WIDTH),
            Horizontal::Left,
            Vertical::Bottom,
            mouse::Interaction::ResizingDiagonallyUp,
        ),
    };
    let target = mouse_area(Space::new().width(width).height(height))
        .on_press(Message::WindowResizeRequested(window, direction))
        .interaction(interaction);

    container(target)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(horizontal)
        .align_y(vertical)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_navigation_stacks_only_when_each_pane_is_narrow() {
        let first = BrowserPaneId(1);
        let second = BrowserPaneId(2);
        assert_eq!(
            pane_navigation_layout(633.0, 170.0, BrowserPaneLayout::Single { active: first }),
            PaneNavigationLayout::StackedRows
        );
        assert_eq!(
            pane_navigation_layout(700.0, 170.0, BrowserPaneLayout::Single { active: first }),
            PaneNavigationLayout::SingleRow
        );
        assert_eq!(
            pane_navigation_layout(
                1_180.0,
                170.0,
                BrowserPaneLayout::Split {
                    axis: SplitAxis::Horizontal,
                    first,
                    second,
                    active: first,
                }
            ),
            PaneNavigationLayout::SingleRow
        );
        assert_eq!(
            pane_navigation_layout(
                900.0,
                170.0,
                BrowserPaneLayout::Split {
                    axis: SplitAxis::Horizontal,
                    first,
                    second,
                    active: first,
                }
            ),
            PaneNavigationLayout::StackedRows
        );
    }
}
