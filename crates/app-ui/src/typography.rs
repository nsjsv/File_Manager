use iced::widget::text;
use iced::Theme;

pub(crate) fn readable_text(
    content: impl ToString,
) -> iced::widget::Text<'static, Theme, iced::Renderer> {
    let content = content.to_string();
    let needs_advanced_shaping = needs_advanced_text_shaping(&content);
    let label = text(content);

    if needs_advanced_shaping {
        label.shaping(iced::widget::text::Shaping::Advanced)
    } else {
        label
    }
}

fn needs_advanced_text_shaping(content: &str) -> bool {
    !content.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::needs_advanced_text_shaping;

    #[test]
    fn ascii_text_keeps_fast_shaping() {
        assert!(!needs_advanced_text_shaping("File Manager"));
    }

    #[test]
    fn chinese_text_uses_advanced_shaping() {
        assert!(needs_advanced_text_shaping("设置"));
    }
}
