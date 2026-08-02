use chrono::{DateTime, Datelike, Duration, TimeZone};
use file_search::{MimePattern, SearchFileKind, SearchFilters, TimeRange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchObjectType {
    Any,
    File,
    Directory,
    Symlink,
}

impl SearchObjectType {
    pub(crate) const ALL: [Self; 4] = [Self::Any, Self::File, Self::Directory, Self::Symlink];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any type",
            Self::File => "Files",
            Self::Directory => "Folders",
            Self::Symlink => "Links",
        }
    }

    fn query_kind(self) -> Option<SearchFileKind> {
        match self {
            Self::Any => None,
            Self::File => Some(SearchFileKind::File),
            Self::Directory => Some(SearchFileKind::Directory),
            Self::Symlink => Some(SearchFileKind::Symlink),
        }
    }

    fn accepts_content_category(self) -> bool {
        matches!(self, Self::Any | Self::File)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchContentCategory {
    Any,
    Documents,
    Images,
    Audio,
    Video,
    Archives,
}

impl SearchContentCategory {
    pub(crate) const ALL: [Self; 6] = [
        Self::Any,
        Self::Documents,
        Self::Images,
        Self::Audio,
        Self::Video,
        Self::Archives,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any content",
            Self::Documents => "Documents",
            Self::Images => "Images",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archives => "Archives",
        }
    }

    fn mime_patterns(self) -> Vec<MimePattern> {
        let exact = |value: &str| MimePattern::Exact(value.to_owned());
        let prefix = |value: &str| MimePattern::Prefix(value.to_owned());
        match self {
            Self::Any => Vec::new(),
            Self::Documents => vec![
                prefix("text/"),
                exact("application/pdf"),
                exact("application/rtf"),
                exact("application/msword"),
                exact("application/vnd.ms-excel"),
                exact("application/vnd.ms-powerpoint"),
                exact("application/vnd.oasis.opendocument.text"),
                exact("application/vnd.oasis.opendocument.spreadsheet"),
                exact("application/vnd.oasis.opendocument.presentation"),
                exact("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                exact("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
                exact("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
            ],
            Self::Images => vec![prefix("image/")],
            Self::Audio => vec![prefix("audio/")],
            Self::Video => vec![prefix("video/")],
            Self::Archives => vec![
                exact("application/zip"),
                exact("application/x-7z-compressed"),
                exact("application/vnd.rar"),
                exact("application/x-rar-compressed"),
                exact("application/x-tar"),
                exact("application/gzip"),
                exact("application/x-bzip2"),
                exact("application/x-xz"),
                exact("application/zstd"),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModifiedTimePreset {
    Any,
    Today,
    PastSevenDays,
    PastThirtyDays,
    ThisYear,
}

impl ModifiedTimePreset {
    pub(crate) const ALL: [Self; 5] = [
        Self::Any,
        Self::Today,
        Self::PastSevenDays,
        Self::PastThirtyDays,
        Self::ThisYear,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any time",
            Self::Today => "Today",
            Self::PastSevenDays => "Past 7 days",
            Self::PastThirtyDays => "Past 30 days",
            Self::ThisYear => "This year",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchFilterPresetState {
    pub(crate) object_type: SearchObjectType,
    pub(crate) content_category: SearchContentCategory,
    pub(crate) modified_time: ModifiedTimePreset,
}

impl Default for SearchFilterPresetState {
    fn default() -> Self {
        Self {
            object_type: SearchObjectType::Any,
            content_category: SearchContentCategory::Any,
            modified_time: ModifiedTimePreset::Any,
        }
    }
}

impl SearchFilterPresetState {
    pub(crate) fn select_object_type(&mut self, object_type: SearchObjectType) {
        self.object_type = object_type;
        if !object_type.accepts_content_category() {
            self.content_category = SearchContentCategory::Any;
        }
    }

    pub(crate) fn select_content_category(&mut self, content_category: SearchContentCategory) {
        self.content_category = content_category;
        if content_category != SearchContentCategory::Any {
            self.object_type = SearchObjectType::File;
        }
    }

    pub(crate) fn query_filters_at<Tz>(self, now: DateTime<Tz>) -> Result<SearchFilters, String>
    where
        Tz: TimeZone,
    {
        Ok(SearchFilters {
            kind: self.object_type.query_kind(),
            mime_patterns: self.content_category.mime_patterns(),
            modified: modified_time_range_at(self.modified_time, now)?,
            accessed: None,
            created: None,
        })
    }
}

fn modified_time_range_at<Tz>(
    preset: ModifiedTimePreset,
    now: DateTime<Tz>,
) -> Result<Option<TimeRange>, String>
where
    Tz: TimeZone,
{
    let end_ms = now.timestamp_millis();
    let start_ms = match preset {
        ModifiedTimePreset::Any => return Ok(None),
        ModifiedTimePreset::PastSevenDays => (now - Duration::days(7)).timestamp_millis(),
        ModifiedTimePreset::PastThirtyDays => (now - Duration::days(30)).timestamp_millis(),
        ModifiedTimePreset::Today => {
            local_calendar_start(&now, now.year(), now.month(), now.day())?
        }
        ModifiedTimePreset::ThisYear => local_calendar_start(&now, now.year(), 1, 1)?,
    };
    Ok(Some(TimeRange { start_ms, end_ms }))
}

fn local_calendar_start<Tz>(
    now: &DateTime<Tz>,
    year: i32,
    month: u32,
    day: u32,
) -> Result<i64, String>
where
    Tz: TimeZone,
{
    now.timezone()
        .with_ymd_and_hms(year, month, day, 0, 0, 0)
        .earliest()
        .map(|start| start.timestamp_millis())
        .ok_or_else(|| {
            format!("local calendar boundary is unavailable for {year:04}-{month:02}-{day:02}")
        })
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use chrono_tz::America::New_York;

    use super::*;

    #[test]
    fn content_category_and_object_type_normalize_to_a_valid_query() {
        let mut filters = SearchFilterPresetState::default();
        filters.select_content_category(SearchContentCategory::Images);
        assert_eq!(filters.object_type, SearchObjectType::File);
        assert_eq!(
            filters
                .query_filters_at(New_York.with_ymd_and_hms(2026, 3, 8, 12, 0, 0).unwrap())
                .unwrap()
                .mime_patterns,
            vec![MimePattern::Prefix("image/".to_owned())]
        );

        filters.select_object_type(SearchObjectType::Directory);
        assert_eq!(filters.content_category, SearchContentCategory::Any);
        assert!(filters
            .query_filters_at(New_York.with_ymd_and_hms(2026, 3, 8, 12, 0, 0).unwrap())
            .unwrap()
            .mime_patterns
            .is_empty());
    }

    #[test]
    fn every_content_category_maps_to_its_representative_mime_type() {
        let cases = [
            (SearchContentCategory::Documents, "application/pdf"),
            (SearchContentCategory::Images, "image/png"),
            (SearchContentCategory::Audio, "audio/flac"),
            (SearchContentCategory::Video, "video/mp4"),
            (SearchContentCategory::Archives, "application/zip"),
        ];
        let now = New_York.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();

        for (category, representative) in cases {
            let mut presets = SearchFilterPresetState::default();
            presets.select_content_category(category);
            let filters = presets.query_filters_at(now.clone()).unwrap();
            assert!(filters.mime_patterns.iter().any(|pattern| match pattern {
                MimePattern::Exact(expected) => expected == representative,
                MimePattern::Prefix(expected) => representative.starts_with(expected),
            }));
            assert_eq!(filters.kind, Some(SearchFileKind::File));
        }
    }

    #[test]
    fn today_uses_local_midnight_across_a_dst_transition() {
        let now = New_York.with_ymd_and_hms(2026, 3, 8, 12, 0, 0).unwrap();
        let range = modified_time_range_at(ModifiedTimePreset::Today, now)
            .unwrap()
            .unwrap();

        assert_eq!(
            range.end_ms - range.start_ms,
            Duration::hours(11).num_milliseconds()
        );
    }

    #[test]
    fn rolling_days_use_fixed_elapsed_duration_across_dst() {
        let now = New_York.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap();
        let range = modified_time_range_at(ModifiedTimePreset::PastSevenDays, now)
            .unwrap()
            .unwrap();

        assert_eq!(
            range.end_ms - range.start_ms,
            Duration::days(7).num_milliseconds()
        );
    }

    #[test]
    fn this_year_starts_at_the_local_calendar_year_boundary() {
        let now = New_York.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let expected = New_York.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let range = modified_time_range_at(ModifiedTimePreset::ThisYear, now)
            .unwrap()
            .unwrap();

        assert_eq!(range.start_ms, expected.timestamp_millis());
        assert_eq!(range.end_ms, now.timestamp_millis());
    }
}
