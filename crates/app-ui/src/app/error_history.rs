use super::FileBrowser;

impl FileBrowser {
    pub(crate) fn current_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn error_history(&self) -> &[String] {
        &self.error_history
    }

    pub(super) fn show_global_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        self.error_history.push(error.clone());
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
    fn global_error_history_keeps_replaced_toast_messages() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        browser.show_global_error("first failure");
        browser.show_global_error("second failure");
        browser.clear_global_error();
        browser.replace_global_error(None);

        let expected = vec!["first failure".to_owned(), "second failure".to_owned()];
        assert_eq!(browser.current_error(), None);
        assert_eq!(browser.error_history(), expected.as_slice());
    }
}
