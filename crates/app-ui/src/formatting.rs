use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

pub(crate) fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(crate) fn format_system_time(time: SystemTime) -> String {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(error) => -(error.duration().as_secs() as i64),
    };
    format_unix_seconds_utc(seconds)
}

fn format_unix_seconds_utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_phase = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_phase + 2) / 5 + 1;
    let month = month_phase + if month_phase < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    (year, month, day)
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
    let start_chars = visible_chars.div_ceil(2);
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
    let start_chars = visible_chars.div_ceil(2);
    let end_chars = visible_chars / 2;
    let mut text = String::with_capacity(max_chars);
    text.push_str(&content[..start_chars]);
    text.push_str(MARKER);
    text.push_str(&content[content.len() - end_chars..]);
    text
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::{format_duration, format_middle_ellipsized_text, format_system_time};

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

    #[test]
    fn formats_audio_duration() {
        assert_eq!(format_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1:01:01");
    }

    #[test]
    fn formats_system_time_as_utc_timestamp() {
        let time = UNIX_EPOCH + Duration::from_secs(1_704_067_200);
        assert_eq!(format_system_time(time), "2024-01-01 00:00:00 UTC");
    }
}
