use std::time::{Duration, Instant};

use iced::Task;

use super::runtime::scrollbar_auto_hide_command;
use super::FileBrowser;
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
pub(crate) struct ScrollbarRegionState {
    visibility: ScrollbarVisibility,
    auto_hide_generation: u64,
    animation: Option<ScrollbarAnimation>,
}

impl Default for ScrollbarRegionState {
    fn default() -> Self {
        Self {
            visibility: ScrollbarVisibility::Hidden,
            auto_hide_generation: 0,
            animation: None,
        }
    }
}

impl FileBrowser {
    pub(super) fn scrollbar_animation_is_active(&self) -> bool {
        self.scrollbar_regions
            .values()
            .any(|region| region.animation.is_some())
    }

    pub(crate) fn scrollbar_visibility_for(&self, region: &ScrollbarRegion) -> ScrollbarVisibility {
        self.scrollbar_regions
            .get(region)
            .map(|region| region.visibility)
            .unwrap_or(ScrollbarVisibility::Hidden)
    }

    pub(super) fn show_scrollbars_temporarily(&mut self, region: ScrollbarRegion) -> Task<Message> {
        let region_state = self.scrollbar_regions.entry(region.clone()).or_default();
        region_state.auto_hide_generation = region_state.auto_hide_generation.wrapping_add(1);

        let current_opacity = region_state.visibility.opacity();
        if (1.0 - current_opacity) <= f32::EPSILON {
            region_state.visibility = ScrollbarVisibility::Visible;
            region_state.animation = None;
        } else if !matches!(
            region_state.animation,
            Some(ScrollbarAnimation::Revealing { .. })
        ) {
            let initial_opacity = current_opacity.max(SCROLLBAR_MIN_REVEAL_OPACITY);
            region_state.visibility = ScrollbarVisibility::with_opacity(initial_opacity);
            region_state.animation = Some(ScrollbarAnimation::Revealing {
                started_at: Instant::now(),
                initial_opacity,
            });
        }

        scrollbar_auto_hide_command(region, region_state.auto_hide_generation)
    }

    pub(super) fn start_scrollbar_hide(&mut self, region: ScrollbarRegion, generation: u64) {
        let Some(region_state) = self.scrollbar_regions.get_mut(&region) else {
            return;
        };
        if region_state.auto_hide_generation != generation {
            return;
        }

        let initial_opacity = region_state.visibility.opacity();
        if initial_opacity <= f32::EPSILON {
            self.scrollbar_regions.remove(&region);
            return;
        }

        region_state.animation = Some(ScrollbarAnimation::Hiding {
            started_at: Instant::now(),
            initial_opacity,
        });
    }

    pub(super) fn advance_scrollbar_animation(&mut self) -> Task<Message> {
        self.scrollbar_regions.retain(|_, region| {
            let Some(animation) = region.animation else {
                return true;
            };

            match animation {
                ScrollbarAnimation::Revealing {
                    started_at,
                    initial_opacity,
                } => advance_scrollbar_reveal(region, started_at, initial_opacity),
                ScrollbarAnimation::Hiding {
                    started_at,
                    initial_opacity,
                } => advance_scrollbar_hide(region, started_at, initial_opacity),
            }
        });

        Task::none()
    }
}

fn advance_scrollbar_reveal(
    region: &mut ScrollbarRegionState,
    started_at: Instant,
    initial_opacity: f32,
) -> bool {
    let progress = scrollbar_animation_progress(started_at, SCROLLBAR_REVEAL_DURATION);
    if progress >= 1.0 {
        region.visibility = ScrollbarVisibility::Visible;
        region.animation = None;
        return true;
    }

    let opacity = initial_opacity + (1.0 - initial_opacity) * ease_out_cubic(progress);
    region.visibility = ScrollbarVisibility::with_opacity(opacity);
    true
}

fn advance_scrollbar_hide(
    region: &mut ScrollbarRegionState,
    started_at: Instant,
    initial_opacity: f32,
) -> bool {
    let progress = scrollbar_animation_progress(started_at, SCROLLBAR_HIDE_DURATION);
    if progress >= 1.0 {
        return false;
    }

    let opacity = initial_opacity * (1.0 - smoothstep(progress));
    region.visibility = ScrollbarVisibility::with_opacity(opacity);
    true
}

fn scrollbar_animation_progress(started_at: Instant, duration: Duration) -> f32 {
    (started_at.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn ease_out_cubic(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn smoothstep(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    progress * progress * (3.0 - 2.0 * progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::model::BrowserPaneId;

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

        let active_region = ScrollbarRegion::PaneList(BrowserPaneId::PRIMARY);
        let inactive_region = ScrollbarRegion::Sidebar;
        drop(browser.show_scrollbars_temporarily(active_region.clone()));

        assert!(browser.scrollbar_visibility_for(&active_region).opacity() > 0.0);
        assert_eq!(
            browser.scrollbar_visibility_for(&inactive_region),
            ScrollbarVisibility::Hidden
        );
    }

    #[test]
    fn changing_scrollbar_region_keeps_previous_region_visible() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        let first_region = ScrollbarRegion::Sidebar;
        let second_region = ScrollbarRegion::PaneList(BrowserPaneId::PRIMARY);
        drop(browser.show_scrollbars_temporarily(first_region.clone()));
        drop(browser.show_scrollbars_temporarily(second_region.clone()));

        assert!(browser.scrollbar_visibility_for(&first_region).opacity() > 0.0);
        assert!(browser.scrollbar_visibility_for(&second_region).opacity() > 0.0);
    }

    #[test]
    fn stale_scrollbar_hide_does_not_hide_region_after_new_scroll() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        let region = ScrollbarRegion::Sidebar;
        drop(browser.show_scrollbars_temporarily(region.clone()));
        drop(browser.show_scrollbars_temporarily(region.clone()));
        browser.start_scrollbar_hide(region.clone(), 1);

        assert!(browser.scrollbar_visibility_for(&region).opacity() > 0.0);
    }

    #[test]
    fn scrollbar_hide_only_affects_its_own_region() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        let first_region = ScrollbarRegion::Sidebar;
        let second_region = ScrollbarRegion::PaneList(BrowserPaneId::PRIMARY);
        drop(browser.show_scrollbars_temporarily(first_region.clone()));
        drop(browser.show_scrollbars_temporarily(second_region.clone()));
        browser.start_scrollbar_hide(first_region.clone(), 1);

        assert!(matches!(
            browser.scrollbar_regions.get(&first_region),
            Some(ScrollbarRegionState {
                animation: Some(ScrollbarAnimation::Hiding { .. }),
                ..
            })
        ));
        assert!(browser.scrollbar_visibility_for(&second_region).opacity() > 0.0);
    }
}
