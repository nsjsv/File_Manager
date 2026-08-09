use std::collections::{BTreeSet, HashSet};

use file_operation_store::{
    StoredApplicationShutdown, StoredBrowserSessionShutdown, TaskQueueStore,
};
use iced::{window, Task};

use super::file_operations::queue_finish_from_completion;
use super::FileBrowser;
use crate::commands::commit_application_shutdown_command;
use crate::model::{snapshot_to_stored, Message};
use crate::operation_history::FileOperationCompletion;
use crate::startup_trace;

enum ShutdownWindowFact {
    CloseRequired,
    AlreadyClosed(window::Id),
}

pub(super) enum ApplicationShutdownPhase {
    Running,
    ClosingWindows,
    Draining(Box<ApplicationShutdownDrain>),
    ExitRequested,
}

pub(super) struct ApplicationShutdownDrain {
    waiting_for_operation_ids: BTreeSet<u64>,
    store: Option<TaskQueueStore>,
    commit: Option<StoredApplicationShutdown>,
    persistence_started: bool,
    pending_window_ids: HashSet<window::Id>,
}

impl ApplicationShutdownPhase {
    pub(super) fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    pub(super) fn is_draining(&self) -> bool {
        matches!(self, Self::Draining(_))
    }
}

impl FileBrowser {
    pub(super) fn begin_application_shutdown(&mut self) -> Task<Message> {
        self.start_application_shutdown(ShutdownWindowFact::CloseRequired)
    }

    fn start_application_shutdown(&mut self, window_fact: ShutdownWindowFact) -> Task<Message> {
        if !self.application_shutdown_phase.is_running() {
            return Task::none();
        }
        startup_trace::mark("close_request_received");
        self.application_shutdown_phase = ApplicationShutdownPhase::ClosingWindows;

        let browser_session = if self.should_save_browser_session() {
            snapshot_to_stored(self.browser_session_snapshot())
                .map(StoredBrowserSessionShutdown::Persist)
                .unwrap_or(StoredBrowserSessionShutdown::Skip)
        } else {
            StoredBrowserSessionShutdown::Skip
        };
        let store = self.operation_queue.task_queue_store().cloned();
        let disposition = self.operation_queue.begin_application_shutdown();
        startup_trace::record_application_shutdown_plan(
            disposition.waiting_for_operation_ids.len(),
            disposition.stopping_signal_count,
            disposition.journal_read_count,
            disposition.interrupted_recoverable_tasks.len(),
            disposition.transient_task_ids.len(),
        );
        startup_trace::mark("shutdown_state_captured");

        self.cancel_work_for_application_shutdown();
        self.pending_browser_session_save = false;
        let mut close_commands = Vec::with_capacity(4);
        let mut pending_window_ids = HashSet::with_capacity(4);
        if let Some(settings_window) = self.settings_window.take() {
            pending_window_ids.insert(settings_window);
            close_commands.push(window::close(settings_window));
        }
        if let Some(properties_window) = self.properties_window.take() {
            pending_window_ids.insert(properties_window);
            close_commands.push(window::close(properties_window));
        }
        if let Some(preview_window) = self.preview_window.take() {
            pending_window_ids.insert(preview_window);
            close_commands.push(window::close(preview_window));
        }
        if !matches!(window_fact, ShutdownWindowFact::AlreadyClosed(window) if window == self.main_window)
        {
            pending_window_ids.insert(self.main_window);
            close_commands.push(window::close(self.main_window));
        }
        startup_trace::mark("window_close_dispatched");
        startup_trace::mark("shutdown_operation_quiescence_requested");

        let waiting_for_operation_ids = disposition.waiting_for_operation_ids;
        let waiting_is_empty = waiting_for_operation_ids.is_empty();
        let pending_windows_are_empty = pending_window_ids.is_empty();
        self.application_shutdown_phase =
            ApplicationShutdownPhase::Draining(Box::new(ApplicationShutdownDrain {
                waiting_for_operation_ids,
                store,
                commit: Some(StoredApplicationShutdown {
                    browser_session,
                    interrupted_recoverable_tasks: disposition.interrupted_recoverable_tasks,
                    transient_task_ids: disposition.transient_task_ids,
                }),
                persistence_started: false,
                pending_window_ids,
            }));
        if waiting_is_empty {
            startup_trace::mark("shutdown_operation_quiescence_finished");
        }
        if pending_windows_are_empty {
            startup_trace::mark("window_close_commands_completed");
            return self.start_application_shutdown_persistence();
        }
        let close_task = match close_commands.len() {
            1 => close_commands.pop().expect("one close command"),
            _ => Task::batch(close_commands),
        };
        close_task.chain(Task::done(Message::ApplicationWindowCloseCommandsFinished))
    }

    pub(super) fn accept_application_window_close_commands_finished(&mut self) -> Task<Message> {
        let ApplicationShutdownPhase::Draining(drain) = &mut self.application_shutdown_phase else {
            return Task::none();
        };
        if drain.pending_window_ids.is_empty() {
            return Task::none();
        }
        drain.pending_window_ids.clear();
        startup_trace::mark("window_close_commands_completed");
        self.start_application_shutdown_persistence()
    }

    pub(super) fn accept_application_window_closed(&mut self, window: window::Id) -> Task<Message> {
        if self.application_shutdown_phase.is_running() {
            if window == self.main_window {
                return self.start_application_shutdown(ShutdownWindowFact::AlreadyClosed(window));
            }
            return self.close_auxiliary_window(window);
        }
        let ApplicationShutdownPhase::Draining(drain) = &mut self.application_shutdown_phase else {
            return Task::none();
        };
        if !drain.pending_window_ids.remove(&window) || !drain.pending_window_ids.is_empty() {
            return Task::none();
        }
        startup_trace::mark("window_close_commands_completed");
        self.start_application_shutdown_persistence()
    }

    pub(super) fn accept_application_shutdown_browser_session_saved(
        &mut self,
        outcome: Result<(), String>,
    ) -> Task<Message> {
        let outcome = self.record_browser_session_save_outcome(outcome);
        if let Err(error) = outcome {
            tracing::warn!(
                target: "app_ui::shutdown",
                error = %error,
                "an in-flight browser session save failed before final shutdown persistence"
            );
        }
        self.start_application_shutdown_persistence()
    }

    pub(super) fn accept_application_shutdown_operation_finished(
        &mut self,
        task_id: u64,
        completion: FileOperationCompletion,
    ) -> Task<Message> {
        let is_waiting = matches!(
            &self.application_shutdown_phase,
            ApplicationShutdownPhase::Draining(drain)
                if drain.waiting_for_operation_ids.contains(&task_id)
        );
        if !is_waiting {
            return Task::none();
        }
        let is_recoverable = self
            .operation_queue
            .operation_uses_recovery_journal(task_id);
        let normal_recoverable_finalized = if is_recoverable
            && !matches!(
                completion,
                FileOperationCompletion::RecoveryInterrupted(_, _)
            ) {
            let (terminal_status, storage_error) =
                self.operation_queue.finish_for_application_shutdown(
                    task_id,
                    queue_finish_from_completion(&completion),
                );
            let storage_succeeded = storage_error.is_none();
            if let Some(error) = &storage_error {
                tracing::error!(
                    target: "app_ui::shutdown",
                    task_id,
                    error = %error,
                    "normal operation completion could not be persisted during shutdown"
                );
            }
            terminal_status.is_some() && storage_succeeded
        } else {
            false
        };

        let ApplicationShutdownPhase::Draining(drain) = &mut self.application_shutdown_phase else {
            return Task::none();
        };
        drain.waiting_for_operation_ids.remove(&task_id);
        if normal_recoverable_finalized {
            if let Some(commit) = &mut drain.commit {
                commit
                    .interrupted_recoverable_tasks
                    .retain(|task| task.task_id != task_id);
            }
        }
        if !drain.waiting_for_operation_ids.is_empty() {
            return Task::none();
        }
        startup_trace::mark("shutdown_operation_quiescence_finished");
        self.start_application_shutdown_persistence()
    }

    pub(super) fn accept_application_shutdown_persisted(
        &mut self,
        outcome: Result<(), String>,
    ) -> Task<Message> {
        let ApplicationShutdownPhase::Draining(drain) = &self.application_shutdown_phase else {
            return Task::none();
        };
        if !drain.persistence_started {
            return Task::none();
        }
        match outcome {
            Ok(()) => startup_trace::mark("shutdown_persistence_finished"),
            Err(error) => {
                startup_trace::mark("shutdown_persistence_failed");
                tracing::error!(
                    target: "app_ui::shutdown",
                    error = %error,
                    "application shutdown persistence failed"
                );
            }
        }
        self.operation_queue
            .release_application_shutdown_ownership();
        self.application_shutdown_phase = ApplicationShutdownPhase::ExitRequested;
        startup_trace::mark("iced_exit_requested");
        iced::exit()
    }

    fn start_application_shutdown_persistence(&mut self) -> Task<Message> {
        let ApplicationShutdownPhase::Draining(drain) = &mut self.application_shutdown_phase else {
            return Task::none();
        };
        if drain.persistence_started
            || !drain.pending_window_ids.is_empty()
            || !drain.waiting_for_operation_ids.is_empty()
            || self.browser_session_saves_in_flight > 0
        {
            return Task::none();
        }
        let Some(commit) = drain.commit.take() else {
            return Task::none();
        };
        drain.persistence_started = true;
        startup_trace::mark("shutdown_persistence_started");
        commit_application_shutdown_command(drain.store.take(), commit)
    }

    fn cancel_work_for_application_shutdown(&mut self) {
        self.system_focused_window = None;
        self.maximized_windows.clear();
        self.discard_search_workspace();
        self.invalidate_startup_directory_validation();
        if let Some(cancellation) = self.directory_load_cancel.take() {
            cancellation.cancel();
        }
        for pane in &mut self.panes {
            if let Some(cancellation) = pane.directory_load_cancel.take() {
                cancellation.cancel();
            }
            for expanded in pane.expanded_directories.values_mut() {
                if let Some(cancellation) = expanded.load_cancel.take() {
                    cancellation.cancel();
                }
            }
        }
        for expanded in self.expanded_directories.values_mut() {
            if let Some(cancellation) = expanded.load_cancel.take() {
                cancellation.cancel();
            }
        }
        self.directory_metadata_in_flight.clear();
        self.clear_file_properties_state();
        self.archive_creation = None;
        self.archive_extraction = None;
        self.cancel_file_drag_interaction();
        self.clear_preview();
        self.pending_preview_resize = None;
    }
}
