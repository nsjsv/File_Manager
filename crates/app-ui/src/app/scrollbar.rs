use std::time::{Duration, Instant};

use iced::Task;

use super::runtime::scrollbar_auto_hide_command;
use super::FileBrowser;
use crate::model::{Message, ScrollbarVisibility};

pub(super) const SCROLLBAR_ANIMATION_INTERVAL: Duration = Duration::from_millis(16);

const SCROLLBAR_REVEAL_DURATION: Duration = Duration::from_millis(96);
const SCROLLBAR_HIDE_DURATION: Duration = Duration::from_millis(300);
const SCROLLBAR_MIN_REVEAL_OPACITY: f32 = 0.12;

#[derive(Debug, Clone, Copy)]
pub(super) enum ScrollbarAnimation {
    Revealing {
        started_at: Instant,
        initial_opacity: f32,
    },
    Hiding {
        started_at: Instant,
        initial_opacity: f32,
    },
}

impl FileBrowser {
    pub(super) fn scrollbar_animation_is_active(&self) -> bool {
        self.scrollbar_animation.is_some()
    }

    pub(super) fn show_scrollbars_temporarily(&mut self) -> Task<Message> {
        self.scrollbar_auto_hide_generation = self.scrollbar_auto_hide_generation.wrapping_add(1);

        let current_opacity = self.scrollbar_visibility.opacity();
        if (1.0 - current_opacity) <= f32::EPSILON {
            self.scrollbar_visibility = ScrollbarVisibility::Visible;
            self.scrollbar_animation = None;
        } else if !matches!(
            self.scrollbar_animation,
            Some(ScrollbarAnimation::Revealing { .. })
        ) {
            let initial_opacity = current_opacity.max(SCROLLBAR_MIN_REVEAL_OPACITY);
            self.scrollbar_visibility = ScrollbarVisibility::with_opacity(initial_opacity);
            self.scrollbar_animation = Some(ScrollbarAnimation::Revealing {
                started_at: Instant::now(),
                initial_opacity,
            });
        }

        scrollbar_auto_hide_command(self.scrollbar_auto_hide_generation)
    }

    pub(super) fn start_scrollbar_hide(&mut self) {
        let initial_opacity = self.scrollbar_visibility.opacity();
        if initial_opacity <= f32::EPSILON {
            self.scrollbar_visibility = ScrollbarVisibility::Hidden;
            self.scrollbar_animation = None;
            return;
        }

        self.scrollbar_animation = Some(ScrollbarAnimation::Hiding {
            started_at: Instant::now(),
            initial_opacity,
        });
    }

    pub(super) fn advance_scrollbar_animation(&mut self) -> Task<Message> {
        let Some(animation) = self.scrollbar_animation else {
            return Task::none();
        };

        match animation {
            ScrollbarAnimation::Revealing {
                started_at,
                initial_opacity,
            } => self.advance_scrollbar_reveal(started_at, initial_opacity),
            ScrollbarAnimation::Hiding {
                started_at,
                initial_opacity,
            } => self.advance_scrollbar_hide(started_at, initial_opacity),
        }

        Task::none()
    }

    fn advance_scrollbar_reveal(&mut self, started_at: Instant, initial_opacity: f32) {
        let progress = scrollbar_animation_progress(started_at, SCROLLBAR_REVEAL_DURATION);
        if progress >= 1.0 {
            self.scrollbar_visibility = ScrollbarVisibility::Visible;
            self.scrollbar_animation = None;
            return;
        }

        let opacity = initial_opacity + (1.0 - initial_opacity) * ease_out_cubic(progress);
        self.scrollbar_visibility = ScrollbarVisibility::with_opacity(opacity);
    }

    fn advance_scrollbar_hide(&mut self, started_at: Instant, initial_opacity: f32) {
        let progress = scrollbar_animation_progress(started_at, SCROLLBAR_HIDE_DURATION);
        if progress >= 1.0 {
            self.scrollbar_visibility = ScrollbarVisibility::Hidden;
            self.scrollbar_animation = None;
            return;
        }

        let opacity = initial_opacity * (1.0 - smoothstep(progress));
        self.scrollbar_visibility = ScrollbarVisibility::with_opacity(opacity);
    }
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
}
