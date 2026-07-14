use super::FileBrowser;
use crate::model::sanitized_application_log_detail;

#[cfg(test)]
std::thread_local! {
    static RECORDED_GLOBAL_ERRORS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn record_global_error(log_error: &str) {
    tracing::error!(
        target: "app_ui::global_error",
        event = "global_error_displayed",
        error = %log_error,
        "global application error displayed"
    );
    #[cfg(test)]
    RECORDED_GLOBAL_ERRORS.with(|count| count.set(count.get() + 1));
}

impl FileBrowser {
    pub(crate) fn current_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn show_global_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        let log_error = sanitized_application_log_detail(&error);
        record_global_error(&log_error);
        self.error = Some(error);
    }

    pub(super) fn clear_global_error(&mut self) {
        self.error = None;
    }

    pub(super) fn replace_global_error(&mut self, error: Option<String>) {
        self.clear_global_error();
        if let Some(error) = error {
            self.show_global_error(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    #[test]
    fn global_error_keeps_only_the_current_toast_and_records_once() {
        RECORDED_GLOBAL_ERRORS.with(|count| count.set(0));
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        browser.show_global_error("smb://alice:secret@example.test/share");
        assert_eq!(
            browser.current_error(),
            Some("smb://alice:secret@example.test/share")
        );
        RECORDED_GLOBAL_ERRORS.with(|count| assert_eq!(count.get(), 1));

        browser.clear_global_error();
        assert_eq!(browser.current_error(), None);
        browser.replace_global_error(Some("replacement failure".to_owned()));
        assert_eq!(browser.current_error(), Some("replacement failure"));
    }
}
