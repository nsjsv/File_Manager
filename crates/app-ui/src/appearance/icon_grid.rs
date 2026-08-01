use iced::{Background, Color, Theme};

use super::{base_text_color, container, is_dark_theme};

pub(crate) fn icon_grid_expansion_panel_style(
    depth: usize,
) -> impl Fn(&Theme) -> container::Appearance + Clone {
    move |theme| {
        let alternate = depth % 2 == 1;
        let background = if is_dark_theme(theme) {
            if alternate {
                Color::from_rgb8(29, 39, 47)
            } else {
                Color::from_rgb8(25, 35, 43)
            }
        } else if alternate {
            Color::from_rgb8(234, 241, 244)
        } else {
            Color::from_rgb8(239, 245, 242)
        };
        container::Appearance {
            background: Some(Background::Color(background)),
            text_color: Some(base_text_color(theme)),
            ..container::Appearance::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expansion_surfaces_use_borderless_distinct_theme_layers() {
        for theme in [Theme::Light, Theme::Dark] {
            let first = icon_grid_expansion_panel_style(1)(&theme);
            let second = icon_grid_expansion_panel_style(2)(&theme);

            assert_eq!(first.border.width, 0.0);
            assert_eq!(second.border.width, 0.0);
            assert_ne!(first.background, second.background);
            assert_eq!(first.text_color, Some(base_text_color(&theme)));
        }
    }
}
