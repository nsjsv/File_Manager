pub(crate) fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub(crate) fn format_middle_ellipsized_text(content: &str, max_chars: usize) -> String {
    const MARKER: &str = "...";

    if content.len() <= max_chars {
        return content.to_owned();
    }

    if content.is_ascii() {
        return format_ascii_middle_ellipsized_text(content, max_chars);
    }

    if content.chars().count() <= max_chars {
        return content.to_owned();
    }

    if max_chars <= MARKER.len() {
        return MARKER.chars().take(max_chars).collect();
    }

    let visible_chars = max_chars - MARKER.len();
    let start_chars = (visible_chars + 1) / 2;
    let end_chars = visible_chars / 2;
    let start: String = content.chars().take(start_chars).collect();
    let end: String = content
        .chars()
        .rev()
        .take(end_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    format!("{start}{MARKER}{end}")
}

fn format_ascii_middle_ellipsized_text(content: &str, max_chars: usize) -> String {
    const MARKER: &str = "...";

    if max_chars <= MARKER.len() {
        return MARKER[..max_chars].to_owned();
    }

    let visible_chars = max_chars - MARKER.len();
    let start_chars = (visible_chars + 1) / 2;
    let end_chars = visible_chars / 2;
    let mut text = String::with_capacity(max_chars);
    text.push_str(&content[..start_chars]);
    text.push_str(MARKER);
    text.push_str(&content[content.len() - end_chars..]);
    text
}

#[cfg(test)]
mod tests {
    use super::format_middle_ellipsized_text;

    #[test]
    fn keeps_short_text() {
        assert_eq!(format_middle_ellipsized_text("Documents", 16), "Documents");
    }

    #[test]
    fn trims_long_text_from_middle() {
        assert_eq!(
            format_middle_ellipsized_text("very-long-directory-name", 12),
            "very-...name"
        );
    }
}
