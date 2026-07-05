use super::FileBrowser;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayedErrorMessage {
    pub(crate) title: Option<&'static str>,
    pub(crate) details: Vec<String>,
    pub(crate) message: String,
}

impl FileBrowser {
    pub(crate) fn current_error(&self) -> Option<&str> {
        self.error.as_deref()
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

    pub(crate) fn displayed_error_messages(&self) -> Vec<DisplayedErrorMessage> {
        let mut messages = self
            .error_history
            .iter()
            .rev()
            .map(|message| DisplayedErrorMessage {
                title: None,
                details: Vec::new(),
                message: message.clone(),
            })
            .collect::<Vec<_>>();

        messages.extend(
            self.operation_queue
                .tasks()
                .iter()
                .rev()
                .filter_map(|task| {
                    let message = task.error.clone()?;
                    let path_lines = task.operation.path_lines();
                    Some(DisplayedErrorMessage {
                        title: Some(task.operation.title()),
                        details: vec![
                            format!("Original: {}", path_lines.original_path),
                            format!("Directory: {}", path_lines.directory_path),
                        ],
                        message,
                    })
                }),
        );

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};
    use file_core::FileOperationVerification;
    use std::path::PathBuf;

    #[test]
    fn global_error_history_keeps_replaced_toast_messages() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());

        browser.show_global_error("first failure");
        browser.show_global_error("second failure");
        browser.clear_global_error();
        browser.replace_global_error(None);

        let expected = vec!["first failure".to_owned(), "second failure".to_owned()];
        assert_eq!(browser.current_error(), None);
        assert_eq!(browser.error_history, expected);
    }

    #[test]
    fn displayed_error_messages_include_failed_file_operation_tasks() {
        let (mut browser, _) = FileBrowser::new(config::default_user_config());
        browser.show_global_error("global failure");
        browser.operation_queue.enqueue(QueuedFileOperation::Copy {
            transfers: vec![QueuedTransfer::new(
                PathBuf::from("/home/yuanming/test/test1"),
                PathBuf::from("/home/yuanming/test/test1-copy"),
            )],
            verification: FileOperationVerification::default(),
        });
        let task_id = browser.operation_queue.tasks().last().unwrap().id;
        browser
            .operation_queue
            .finish(task_id, Err("copy failed".to_owned()));

        let messages = browser.displayed_error_messages();

        assert_eq!(messages[0].message, "global failure");
        assert_eq!(messages[1].title.as_deref(), Some("Copy"));
        assert_eq!(
            messages[1].details[0],
            "Original: /home/yuanming/test/test1"
        );
        assert_eq!(messages[1].details[1], "Directory: /home/yuanming/test");
        assert_eq!(messages[1].message, "copy failed");
    }
}
