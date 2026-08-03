use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone};
use file_search::{
    MimePattern, SearchEntryTypeRule, SearchFileKind, SearchFilters, SearchTextScope, TimeRange,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchEntryTypePreset {
    Spreadsheets,
    Video,
    Images,
    Text,
    Documents,
    Folders,
    Audio,
    Pdf,
    Files,
    Archives,
    Links,
}

impl SearchEntryTypePreset {
    pub(crate) const COMMON: [Self; 8] = [
        Self::Spreadsheets,
        Self::Video,
        Self::Images,
        Self::Text,
        Self::Documents,
        Self::Folders,
        Self::Audio,
        Self::Pdf,
    ];
    pub(crate) const MORE: [Self; 3] = [Self::Files, Self::Archives, Self::Links];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Spreadsheets => "Spreadsheets",
            Self::Video => "Video",
            Self::Images => "Images",
            Self::Text => "Text",
            Self::Documents => "Documents",
            Self::Folders => "Folders",
            Self::Audio => "Audio",
            Self::Pdf => "PDF",
            Self::Files => "Files",
            Self::Archives => "Archives",
            Self::Links => "Links",
        }
    }

    fn query_rules(self) -> Vec<SearchEntryTypeRule> {
        let kind = |value| SearchEntryTypeRule::Kind(value);
        let exact = |value: &str| SearchEntryTypeRule::Mime(MimePattern::Exact(value.to_owned()));
        let prefix = |value: &str| SearchEntryTypeRule::Mime(MimePattern::Prefix(value.to_owned()));
        match self {
            Self::Spreadsheets => vec![
                exact("application/vnd.ms-excel"),
                exact("application/vnd.oasis.opendocument.spreadsheet"),
                exact("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
            ],
            Self::Video => vec![prefix("video/")],
            Self::Images => vec![prefix("image/")],
            Self::Text => vec![prefix("text/")],
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
            Self::Folders => vec![kind(SearchFileKind::Directory)],
            Self::Audio => vec![prefix("audio/")],
            Self::Pdf => vec![exact("application/pdf")],
            Self::Files => vec![kind(SearchFileKind::File)],
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
            Self::Links => vec![kind(SearchFileKind::Symlink)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchDateField {
    Accessed,
    Modified,
    Created,
}

impl SearchDateField {
    pub(crate) const ALL: [Self; 3] = [Self::Accessed, Self::Modified, Self::Created];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Accessed => "Accessed",
            Self::Modified => "Modified",
            Self::Created => "Created",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchDatePreset {
    Any,
    Today,
    Yesterday,
    PastSevenDays,
    PastThirtyDays,
    PastYear,
}

impl SearchDatePreset {
    pub(crate) const ALL: [Self; 6] = [
        Self::Any,
        Self::Today,
        Self::Yesterday,
        Self::PastSevenDays,
        Self::PastThirtyDays,
        Self::PastYear,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any time",
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::PastSevenDays => "Past 7 days",
            Self::PastThirtyDays => "Past 30 days",
            Self::PastYear => "Past year",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchFilterPresetState {
    pub(crate) selected_entry_types: Vec<SearchEntryTypePreset>,
    pub(crate) text_scope: SearchTextScope,
    pub(crate) date_field: SearchDateField,
    pub(crate) date_preset: SearchDatePreset,
}

impl Default for SearchFilterPresetState {
    fn default() -> Self {
        Self {
            selected_entry_types: Vec::new(),
            text_scope: SearchTextScope::NameAndContent,
            date_field: SearchDateField::Modified,
            date_preset: SearchDatePreset::Any,
        }
    }
}

impl SearchFilterPresetState {
    pub(crate) fn toggle_entry_type(&mut self, entry_type: SearchEntryTypePreset) {
        if let Some(index) = self
            .selected_entry_types
            .iter()
            .position(|selected| *selected == entry_type)
        {
            self.selected_entry_types.remove(index);
        } else {
            self.selected_entry_types.push(entry_type);
        }
    }

    pub(crate) fn entry_type_is_selected(&self, entry_type: SearchEntryTypePreset) -> bool {
        self.selected_entry_types.contains(&entry_type)
    }

    pub(crate) fn selected_more_type_count(&self) -> usize {
        SearchEntryTypePreset::MORE
            .iter()
            .filter(|entry_type| self.entry_type_is_selected(**entry_type))
            .count()
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn query_filters_at<Tz>(&self, now: DateTime<Tz>) -> Result<SearchFilters, String>
    where
        Tz: TimeZone,
    {
        let mut entry_type_rules = Vec::new();
        for entry_type in &self.selected_entry_types {
            for rule in entry_type.query_rules() {
                if !entry_type_rules.contains(&rule) {
                    entry_type_rules.push(rule);
                }
            }
        }
        let date_range = date_range_at(self.date_preset, now)?;
        let (accessed, modified, created) = match (self.date_field, date_range) {
            (_, None) => (None, None, None),
            (SearchDateField::Accessed, range) => (range, None, None),
            (SearchDateField::Modified, range) => (None, range, None),
            (SearchDateField::Created, range) => (None, None, range),
        };
        Ok(SearchFilters {
            entry_type_rules,
            modified,
            accessed,
            created,
        })
    }
}

fn date_range_at<Tz>(
    preset: SearchDatePreset,
    now: DateTime<Tz>,
) -> Result<Option<TimeRange>, String>
where
    Tz: TimeZone,
{
    let end_ms = now.timestamp_millis();
    let start_ms = match preset {
        SearchDatePreset::Any => return Ok(None),
        SearchDatePreset::PastSevenDays => (now - Duration::days(7)).timestamp_millis(),
        SearchDatePreset::PastThirtyDays => (now - Duration::days(30)).timestamp_millis(),
        SearchDatePreset::PastYear => (now - Duration::days(365)).timestamp_millis(),
        SearchDatePreset::Today => local_calendar_start(&now, now.date_naive())?,
        SearchDatePreset::Yesterday => {
            let today = now.date_naive();
            let yesterday = today
                .pred_opt()
                .ok_or_else(|| "previous local calendar day is unavailable".to_owned())?;
            let today_start_ms = local_calendar_start(&now, today)?;
            return Ok(Some(TimeRange {
                start_ms: local_calendar_start(&now, yesterday)?,
                end_ms: today_start_ms.saturating_sub(1),
            }));
        }
    };
    Ok(Some(TimeRange { start_ms, end_ms }))
}

fn local_calendar_start<Tz>(now: &DateTime<Tz>, date: NaiveDate) -> Result<i64, String>
where
    Tz: TimeZone,
{
    now.timezone()
        .with_ymd_and_hms(date.year(), date.month(), date.day(), 0, 0, 0)
        .earliest()
        .map(|start| start.timestamp_millis())
        .ok_or_else(|| format!("local calendar boundary is unavailable for {date}"))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use chrono_tz::America::New_York;

    use super::*;

    #[test]
    fn entry_types_compose_across_kind_and_mime_without_mutating_each_other() {
        let mut presets = SearchFilterPresetState::default();
        presets.toggle_entry_type(SearchEntryTypePreset::Folders);
        presets.toggle_entry_type(SearchEntryTypePreset::Images);
        presets.toggle_entry_type(SearchEntryTypePreset::Pdf);

        let filters = presets
            .query_filters_at(New_York.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap())
            .unwrap();

        assert!(filters
            .entry_type_rules
            .contains(&SearchEntryTypeRule::Kind(SearchFileKind::Directory)));
        assert!(filters
            .entry_type_rules
            .contains(&SearchEntryTypeRule::Mime(MimePattern::Prefix(
                "image/".to_owned()
            ))));
        assert!(filters
            .entry_type_rules
            .contains(&SearchEntryTypeRule::Mime(MimePattern::Exact(
                "application/pdf".to_owned()
            ))));
        presets.toggle_entry_type(SearchEntryTypePreset::Images);
        assert!(!presets.entry_type_is_selected(SearchEntryTypePreset::Images));
        assert!(presets.entry_type_is_selected(SearchEntryTypePreset::Folders));
    }

    #[test]
    fn overlapping_type_presets_deduplicate_query_rules() {
        let mut presets = SearchFilterPresetState::default();
        for entry_type in [
            SearchEntryTypePreset::Documents,
            SearchEntryTypePreset::Spreadsheets,
            SearchEntryTypePreset::Text,
            SearchEntryTypePreset::Pdf,
        ] {
            presets.toggle_entry_type(entry_type);
        }

        let filters = presets
            .query_filters_at(New_York.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap())
            .unwrap();

        for expected in [
            SearchEntryTypeRule::Mime(MimePattern::Prefix("text/".to_owned())),
            SearchEntryTypeRule::Mime(MimePattern::Exact("application/pdf".to_owned())),
            SearchEntryTypeRule::Mime(MimePattern::Exact("application/vnd.ms-excel".to_owned())),
        ] {
            assert_eq!(
                filters
                    .entry_type_rules
                    .iter()
                    .filter(|rule| **rule == expected)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn every_entry_type_preset_maps_to_an_expected_query_rule() {
        let cases = [
            (
                SearchEntryTypePreset::Spreadsheets,
                SearchEntryTypeRule::Mime(MimePattern::Exact(
                    "application/vnd.ms-excel".to_owned(),
                )),
            ),
            (
                SearchEntryTypePreset::Video,
                SearchEntryTypeRule::Mime(MimePattern::Prefix("video/".to_owned())),
            ),
            (
                SearchEntryTypePreset::Images,
                SearchEntryTypeRule::Mime(MimePattern::Prefix("image/".to_owned())),
            ),
            (
                SearchEntryTypePreset::Text,
                SearchEntryTypeRule::Mime(MimePattern::Prefix("text/".to_owned())),
            ),
            (
                SearchEntryTypePreset::Documents,
                SearchEntryTypeRule::Mime(MimePattern::Exact("application/pdf".to_owned())),
            ),
            (
                SearchEntryTypePreset::Folders,
                SearchEntryTypeRule::Kind(SearchFileKind::Directory),
            ),
            (
                SearchEntryTypePreset::Audio,
                SearchEntryTypeRule::Mime(MimePattern::Prefix("audio/".to_owned())),
            ),
            (
                SearchEntryTypePreset::Pdf,
                SearchEntryTypeRule::Mime(MimePattern::Exact("application/pdf".to_owned())),
            ),
            (
                SearchEntryTypePreset::Files,
                SearchEntryTypeRule::Kind(SearchFileKind::File),
            ),
            (
                SearchEntryTypePreset::Archives,
                SearchEntryTypeRule::Mime(MimePattern::Exact("application/zip".to_owned())),
            ),
            (
                SearchEntryTypePreset::Links,
                SearchEntryTypeRule::Kind(SearchFileKind::Symlink),
            ),
        ];

        for (preset, expected_rule) in cases {
            assert!(preset.query_rules().contains(&expected_rule), "{preset:?}");
        }
    }

    #[test]
    fn selected_date_field_is_the_only_time_constraint() {
        let now = New_York.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap();
        for date_field in SearchDateField::ALL {
            let presets = SearchFilterPresetState {
                date_field,
                date_preset: SearchDatePreset::Today,
                ..SearchFilterPresetState::default()
            };
            let filters = presets.query_filters_at(now).unwrap();

            assert_eq!(
                filters.accessed.is_some(),
                date_field == SearchDateField::Accessed
            );
            assert_eq!(
                filters.modified.is_some(),
                date_field == SearchDateField::Modified
            );
            assert_eq!(
                filters.created.is_some(),
                date_field == SearchDateField::Created
            );
        }
    }

    #[test]
    fn reset_restores_one_default_query_state() {
        let mut presets = SearchFilterPresetState {
            selected_entry_types: vec![SearchEntryTypePreset::Folders],
            text_scope: SearchTextScope::NameOnly,
            date_field: SearchDateField::Created,
            date_preset: SearchDatePreset::PastYear,
        };

        presets.reset();

        assert!(presets.is_default());
        let filters = presets
            .query_filters_at(New_York.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap())
            .unwrap();
        assert!(filters.entry_type_rules.is_empty());
        assert!(filters.modified.is_none());
        assert!(filters.accessed.is_none());
        assert!(filters.created.is_none());
    }

    #[test]
    fn today_uses_local_midnight_across_a_dst_transition() {
        let now = New_York.with_ymd_and_hms(2026, 3, 8, 12, 0, 0).unwrap();
        let range = date_range_at(SearchDatePreset::Today, now)
            .unwrap()
            .unwrap();

        assert_eq!(
            range.end_ms - range.start_ms,
            Duration::hours(11).num_milliseconds()
        );
    }

    #[test]
    fn yesterday_uses_the_complete_local_calendar_day_across_dst() {
        let now = New_York.with_ymd_and_hms(2026, 3, 9, 12, 0, 0).unwrap();
        let expected_start = New_York.with_ymd_and_hms(2026, 3, 8, 0, 0, 0).unwrap();
        let expected_end = New_York
            .with_ymd_and_hms(2026, 3, 9, 0, 0, 0)
            .unwrap()
            .timestamp_millis()
            - 1;
        let range = date_range_at(SearchDatePreset::Yesterday, now)
            .unwrap()
            .unwrap();

        assert_eq!(range.start_ms, expected_start.timestamp_millis());
        assert_eq!(range.end_ms, expected_end);
    }

    #[test]
    fn rolling_presets_use_fixed_elapsed_duration_across_dst() {
        let now = New_York.with_ymd_and_hms(2026, 3, 10, 12, 0, 0).unwrap();
        for (preset, days) in [
            (SearchDatePreset::PastSevenDays, 7),
            (SearchDatePreset::PastThirtyDays, 30),
            (SearchDatePreset::PastYear, 365),
        ] {
            let range = date_range_at(preset, now).unwrap().unwrap();
            assert_eq!(
                range.end_ms - range.start_ms,
                Duration::days(days).num_milliseconds()
            );
        }
    }
}
