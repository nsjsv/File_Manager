use std::time::{Duration, Instant};

use iced::Task;

use super::runtime::scrollbar_auto_hide_command;
use super::FileBrowser;
use crate::animation::{ease_out_cubic, elapsed_fraction as scrollbar_animation_progress};
use crate::model::{Message, ScrollbarRegion, ScrollbarVisibility};

pub(super) const SCROLLBAR_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);

const SCROLLBAR_REVEAL_DURATION: Duration = Duration::from_millis(96);
const SCROLLBAR_HIDE_DURATION: Duration = Duration::from_millis(300);
const SCROLLBAR_MIN_REVEAL_OPACITY: f32 = 0.12;

#[derive(Debug, Clone, Copy)]
enum ScrollbarAnimation {
    Revealing {
        started_at: Instant,
        initial_opacity: f32,
    },
    Hiding {
        started_at: Instant,
        initial_opacity: f32,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct ScrollbarState {
    active_region: Option<ScrollbarRegion>,
    visibility: ScrollbarVisibility,
    auto_hide_generation: u64,
    animation: Option<ScrollbarAnimation>,
}

impl Default for ScrollbarState {
    fn default() -> Self {
        Self {
            active_region: None,
            visibility: ScrollbarVisibility::Hidden,
            auto_hide_generation: 0,
            animation: None,
        }
    }
}

impl FileBrowser {
    pub(super) fn scrollbar_animation_is_active(&self) -> bool {
        self.scrollbar.animation.is_some()
    }

    pub(crate) fn scrollbar_visibility_for(&self, region: &ScrollbarRegion) -> ScrollbarVisibility {
        if self.scrollbar.active_region.as_ref() == Some(region) {
            self.scrollbar.visibility
        } else {
            ScrollbarVisibility::Hidden
        }
    }

    pub(super) fn show_scrollbars_temporarily(&mut self, region: ScrollbarRegion) -> Task<Message> {
        let scrollbar = &mut self.scrollbar;
        scrollbar.active_region = Some(region);
        scrollbar.auto_hide_generation = scrollbar.auto_hide_generation.wrapping_add(1);

        let current_opacity = scrollbar.visibility.opacity();
        if (1.0 - current_opacity) <= f32::EPSILON {
            scrollbar.visibility = ScrollbarVisibility::Visible;
            scrollbar.animation = None;
        } else if !matches!(
            scrollbar.animation,
            Some(ScrollbarAnimation::Revealing { .. })
        ) {
            let initial_opacity = current_opacity.max(SCROLLBAR_MIN_REVEAL_OPACITY);
            scrollbar.visibility = ScrollbarVisibility::with_opacity(initial_opacity);
            scrollbar.animation = Some(ScrollbarAnimation::Revealing {
                started_at: Instant::now(),
                initial_opacity,
            });
        }

        scrollbar_auto_hide_command(scrollbar.auto_hide_generation)
    }

    pub(super) fn start_global_scrollbar_hide(&mut self, generation: u64) {
        if self.scrollbar.auto_hide_generation != generation {
            return;
        }

        let initial_opacity = self.scrollbar.visibility.opacity();
        if initial_opacity <= f32::EPSILON {
            self.scrollbar.active_region = None;
            self.scrollbar.visibility = ScrollbarVisibility::Hidden;
            self.scrollbar.animation = None;
            return;
        }

        self.scrollbar.animation = Some(ScrollbarAnimation::Hiding {
            started_at: Instant::now(),
            initial_opacity,
        });
    }

    pub(super) fn advance_scrollbar_animation(&mut self) -> Task<Message> {
        let Some(animation) = self.scrollbar.animation else {
            return Task::none();
        };

        let still_active = match animation {
            ScrollbarAnimation::Revealing {
                started_at,
                initial_opacity,
            } => advance_scrollbar_reveal(&mut self.scrollbar, started_at, initial_opacity),
            ScrollbarAnimation::Hiding {
                started_at,
                initial_opacity,
            } => advance_scrollbar_hide(&mut self.scrollbar, started_at, initial_opacity),
        };

        if !still_active {
            self.scrollbar.active_region = None;
            self.scrollbar.visibility = ScrollbarVisibility::Hidden;
            self.scrollbar.animation = None;
        }

        Task::none()
    }
}

fn advance_scrollbar_reveal(
    scrollbar: &mut ScrollbarState,
    started_at: Instant,
    initial_opacity: f32,
) -> bool {
    let progress = scrollbar_animation_progress(started_at, SCROLLBAR_REVEAL_DURATION);
    if progress >= 1.0 {
        scrollbar.visibility = ScrollbarVisibility::Visible;
        scrollbar.animation = None;
        return true;
    }

    let opacity = initial_opacity + (1.0 - initial_opacity) * ease_out_cubic(progress);
    scrollbar.visibility = ScrollbarVisibility::with_opacity(opacity);
    true
}

fn advance_scrollbar_hide(
    scrollbar: &mut ScrollbarState,
    started_at: Instant,
    initial_opacity: f32,
) -> bool {
    let progress = scrollbar_animation_progress(started_at, SCROLLBAR_HIDE_DURATION);
    if progress >= 1.0 {
        return false;
    }

    let opacity = initial_opacity * (1.0 - smoothstep(progress));
    scrollbar.visibility = ScrollbarVisibility::with_opacity(opacity);
    true
}

fn smoothstep(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn reveal_curve_starts_fast_and_finishes_at_one() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert!(ease_out_cubic(0.35) > 0.65);
        assert_eq!(ease_out_cubic(1.0), 1.0);
    }

    #[test]
    fn hide_curve_keeps_endpoints_stable() {
        assert_eq!(smoothstep(0.0), 0.0);
        assert!((smoothstep(0.5) - 0.5).abs() <= f32::EPSILON);
        assert_eq!(smoothstep(1.0), 1.0);
    }

    #[test]
    fn scrollbar_visibility_only_applies_to_active_region() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let active_region = ScrollbarRegion::Sidebar;
        let inactive_region = ScrollbarRegion::Settings;

        drop(browser.show_scrollbars_temporarily(active_region.clone()));

        assert!(browser.scrollbar_visibility_for(&active_region).opacity() > 0.0);
        assert_eq!(
            browser.scrollbar_visibility_for(&inactive_region),
            ScrollbarVisibility::Hidden
        );
    }

    #[test]
    fn changing_scrollbar_region_moves_active_visibility() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let first_region = ScrollbarRegion::Sidebar;
        let second_region = ScrollbarRegion::Settings;

        drop(browser.show_scrollbars_temporarily(first_region.clone()));
        drop(browser.show_scrollbars_temporarily(second_region.clone()));

        assert_eq!(
            browser.scrollbar_visibility_for(&first_region),
            ScrollbarVisibility::Hidden
        );
        assert!(browser.scrollbar_visibility_for(&second_region).opacity() > 0.0);
    }

    #[test]
    fn stale_scrollbar_hide_does_not_hide_region_after_new_scroll() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let region = ScrollbarRegion::Sidebar;

        drop(browser.show_scrollbars_temporarily(region.clone()));
        drop(browser.show_scrollbars_temporarily(region.clone()));
        browser.start_global_scrollbar_hide(1);

        assert!(browser.scrollbar_visibility_for(&region).opacity() > 0.0);
    }

    #[test]
    fn scrollbar_hide_uses_global_generation() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        drop(browser.show_scrollbars_temporarily(ScrollbarRegion::Sidebar));
        browser.start_global_scrollbar_hide(1);

        assert!(matches!(
            browser.scrollbar.animation,
            Some(ScrollbarAnimation::Hiding { .. })
        ));
    }

    #[test]
    fn icon_grid_uses_global_auto_hide_scrollbar_state() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        let region = ScrollbarRegion::PaneIcons(crate::model::BrowserPaneId::PRIMARY);

        drop(browser.show_scrollbars_temporarily(region.clone()));

        assert!(browser.scrollbar_visibility_for(&region).opacity() > 0.0);
        assert_eq!(
            browser.scrollbar_visibility_for(&ScrollbarRegion::Sidebar),
            ScrollbarVisibility::Hidden
        );
    }
}
