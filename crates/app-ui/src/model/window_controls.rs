use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WindowControlKind {
    Minimize,
    MaximizeRestore,
    Close,
}

impl WindowControlKind {
    pub(crate) const ALL: [Self; 3] = [Self::Minimize, Self::MaximizeRestore, Self::Close];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::MaximizeRestore => "Maximize / Restore",
            Self::Close => "Close",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::MaximizeRestore => "maximize_restore",
            Self::Close => "close",
        }
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "minimize" => Some(Self::Minimize),
            "maximize_restore" => Some(Self::MaximizeRestore),
            "close" => Some(Self::Close),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowControlSide {
    Left,
    Right,
}

impl WindowControlSide {
    pub(crate) const ALL: [Self; 2] = [Self::Left, Self::Right];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowControlVisibility {
    Visible,
    Hidden,
}

impl WindowControlVisibility {
    pub(crate) fn is_visible(self) -> bool {
        self == Self::Visible
    }

    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Visible => Self::Hidden,
            Self::Hidden => Self::Visible,
        }
    }
}

impl From<bool> for WindowControlVisibility {
    fn from(visible: bool) -> Self {
        if visible {
            Self::Visible
        } else {
            Self::Hidden
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowFrameState {
    Restored,
    Maximized,
}

pub(crate) const WINDOW_TITLE_BAR_HEIGHT: f32 = 40.0;
pub(crate) const WINDOW_TOP_BAR_HEIGHT: f32 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowChromeLayout {
    IntegratedNavigation,
    SeparateTitleBar,
}

impl WindowChromeLayout {
    pub(crate) const ALL: [Self; 2] = [Self::IntegratedNavigation, Self::SeparateTitleBar];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::IntegratedNavigation => "Integrated navigation",
            Self::SeparateTitleBar => "Separate title bar",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::IntegratedNavigation => "integrated_navigation",
            Self::SeparateTitleBar => "separate_title_bar",
        }
    }

    pub(crate) fn from_config_value(value: &str) -> Option<Self> {
        match value {
            "integrated_navigation" => Some(Self::IntegratedNavigation),
            "separate_title_bar" => Some(Self::SeparateTitleBar),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowControlPlacement {
    kind: WindowControlKind,
    side: WindowControlSide,
    visibility: WindowControlVisibility,
}

impl WindowControlPlacement {
    pub(crate) fn new(
        kind: WindowControlKind,
        side: WindowControlSide,
        visibility: WindowControlVisibility,
    ) -> Self {
        Self {
            kind,
            side,
            visibility: normalized_visibility(kind, visibility),
        }
    }

    pub(crate) fn kind(self) -> WindowControlKind {
        self.kind
    }

    pub(crate) fn side(self) -> WindowControlSide {
        self.side
    }

    pub(crate) fn visibility(self) -> WindowControlVisibility {
        self.visibility
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WindowControlsConfig {
    layout: WindowChromeLayout,
    placements: Vec<WindowControlPlacement>,
}

impl WindowControlsConfig {
    pub(crate) fn from_partial_placements(
        layout: WindowChromeLayout,
        placements: Vec<WindowControlPlacement>,
    ) -> Self {
        let mut seen = HashSet::with_capacity(WindowControlKind::ALL.len());
        let mut normalized = Vec::with_capacity(WindowControlKind::ALL.len());
        for placement in placements {
            if seen.insert(placement.kind) {
                normalized.push(WindowControlPlacement::new(
                    placement.kind,
                    placement.side,
                    placement.visibility,
                ));
            }
        }
        for kind in WindowControlKind::ALL {
            if seen.insert(kind) {
                normalized.push(default_placement(kind));
            }
        }
        Self {
            layout,
            placements: normalized,
        }
    }

    pub(crate) fn layout(&self) -> WindowChromeLayout {
        self.layout
    }

    pub(crate) fn placements(&self) -> &[WindowControlPlacement] {
        &self.placements
    }

    pub(crate) fn placements_on(
        &self,
        side: WindowControlSide,
    ) -> impl Iterator<Item = WindowControlPlacement> + '_ {
        self.placements
            .iter()
            .copied()
            .filter(move |placement| placement.side == side)
    }

    pub(crate) fn placement(&self, kind: WindowControlKind) -> WindowControlPlacement {
        self.placements
            .iter()
            .copied()
            .find(|placement| placement.kind == kind)
            .expect("normalized window controls contain every kind")
    }

    pub(crate) fn select_layout(&mut self, layout: WindowChromeLayout) -> bool {
        if self.layout == layout {
            return false;
        }
        self.layout = layout;
        true
    }

    pub(crate) fn set_visibility(
        &mut self,
        kind: WindowControlKind,
        visibility: WindowControlVisibility,
    ) -> bool {
        let normalized = normalized_visibility(kind, visibility);
        let placement = self
            .placements
            .iter_mut()
            .find(|placement| placement.kind == kind)
            .expect("normalized window controls contain every kind");
        if placement.visibility == normalized {
            return false;
        }
        placement.visibility = normalized;
        true
    }

    pub(crate) fn move_to_side(
        &mut self,
        kind: WindowControlKind,
        side: WindowControlSide,
    ) -> bool {
        let index = self
            .placements
            .iter()
            .position(|placement| placement.kind == kind)
            .expect("normalized window controls contain every kind");
        if self.placements[index].side == side {
            return false;
        }
        let mut placement = self.placements.remove(index);
        placement.side = side;
        self.placements.push(placement);
        true
    }

    pub(crate) fn move_before_on_same_side(
        &mut self,
        kind: WindowControlKind,
        target: WindowControlKind,
    ) -> bool {
        if kind == target {
            return false;
        }
        let kind_index = self
            .placements
            .iter()
            .position(|placement| placement.kind == kind)
            .expect("normalized window controls contain every kind");
        let target_index = self
            .placements
            .iter()
            .position(|placement| placement.kind == target)
            .expect("normalized window controls contain every kind");
        if self.placements[kind_index].side != self.placements[target_index].side {
            return false;
        }

        let placement = self.placements.remove(kind_index);
        let insertion_index = self
            .placements
            .iter()
            .position(|candidate| candidate.kind == target)
            .expect("target remains after removing a different control");
        self.placements.insert(insertion_index, placement);
        kind_index != insertion_index
    }

    pub(crate) fn reset(&mut self) -> bool {
        let default = Self::default();
        if *self == default {
            return false;
        }
        *self = default;
        true
    }
}

impl Default for WindowControlsConfig {
    fn default() -> Self {
        Self {
            layout: WindowChromeLayout::IntegratedNavigation,
            placements: WindowControlKind::ALL
                .into_iter()
                .map(default_placement)
                .collect(),
        }
    }
}

fn default_placement(kind: WindowControlKind) -> WindowControlPlacement {
    WindowControlPlacement::new(
        kind,
        WindowControlSide::Right,
        WindowControlVisibility::Visible,
    )
}

fn normalized_visibility(
    kind: WindowControlKind,
    visibility: WindowControlVisibility,
) -> WindowControlVisibility {
    if kind == WindowControlKind::Close {
        WindowControlVisibility::Visible
    } else {
        visibility
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_on(config: &WindowControlsConfig, side: WindowControlSide) -> Vec<WindowControlKind> {
        config
            .placements_on(side)
            .map(WindowControlPlacement::kind)
            .collect()
    }

    #[test]
    fn default_uses_integrated_right_standard_order() {
        let config = WindowControlsConfig::default();

        assert_eq!(config.layout(), WindowChromeLayout::IntegratedNavigation);
        assert!(kinds_on(&config, WindowControlSide::Left).is_empty());
        assert_eq!(
            kinds_on(&config, WindowControlSide::Right),
            WindowControlKind::ALL
        );
        assert!(config
            .placements()
            .iter()
            .all(|placement| placement.visibility().is_visible()));
    }

    #[test]
    fn partial_placements_drop_duplicates_fill_missing_and_keep_close_visible() {
        let config = WindowControlsConfig::from_partial_placements(
            WindowChromeLayout::SeparateTitleBar,
            vec![
                WindowControlPlacement::new(
                    WindowControlKind::Close,
                    WindowControlSide::Left,
                    WindowControlVisibility::Hidden,
                ),
                WindowControlPlacement::new(
                    WindowControlKind::Close,
                    WindowControlSide::Right,
                    WindowControlVisibility::Visible,
                ),
                WindowControlPlacement::new(
                    WindowControlKind::Minimize,
                    WindowControlSide::Left,
                    WindowControlVisibility::Hidden,
                ),
            ],
        );

        assert_eq!(config.placements().len(), 3);
        assert_eq!(
            config.placement(WindowControlKind::Close).side(),
            WindowControlSide::Left
        );
        assert!(config
            .placement(WindowControlKind::Close)
            .visibility()
            .is_visible());
        assert_eq!(
            config.placement(WindowControlKind::MaximizeRestore).side(),
            WindowControlSide::Right
        );
    }

    #[test]
    fn moving_to_side_appends_to_target_side() {
        let mut config = WindowControlsConfig::default();

        assert!(config.move_to_side(WindowControlKind::MaximizeRestore, WindowControlSide::Left));
        assert!(config.move_to_side(WindowControlKind::Minimize, WindowControlSide::Left));

        assert_eq!(
            kinds_on(&config, WindowControlSide::Left),
            vec![
                WindowControlKind::MaximizeRestore,
                WindowControlKind::Minimize,
            ]
        );
        assert_eq!(
            kinds_on(&config, WindowControlSide::Right),
            vec![WindowControlKind::Close]
        );
    }

    #[test]
    fn reorder_accepts_only_same_side_target() {
        let mut config = WindowControlsConfig::default();
        assert!(config.move_to_side(WindowControlKind::Close, WindowControlSide::Left));
        assert!(
            !config.move_before_on_same_side(WindowControlKind::Minimize, WindowControlKind::Close)
        );

        assert!(
            !config.move_before_on_same_side(WindowControlKind::Close, WindowControlKind::Close)
        );
        assert!(config.move_to_side(WindowControlKind::Minimize, WindowControlSide::Left));
        assert!(
            config.move_before_on_same_side(WindowControlKind::Minimize, WindowControlKind::Close)
        );
        assert_eq!(
            kinds_on(&config, WindowControlSide::Left),
            vec![WindowControlKind::Minimize, WindowControlKind::Close]
        );
    }

    #[test]
    fn close_visibility_cannot_be_hidden() {
        let mut config = WindowControlsConfig::default();

        assert!(!config.set_visibility(WindowControlKind::Close, WindowControlVisibility::Hidden));
        assert!(config
            .placement(WindowControlKind::Close)
            .visibility()
            .is_visible());
    }
}
