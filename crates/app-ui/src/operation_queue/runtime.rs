use super::*;

impl FileOperationQueue {
    pub(super) fn start_next(&mut self) -> Option<String> {
        if self.active_subscription().is_some() {
            return None;
        }
        if let Some(position) = self
            .tasks
            .iter()
            .find(|task| task.status == FileOperationStatus::Pending)
            .map(|task| task.id)
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
        {
            let task = &mut self.tasks[position];
            task.status = FileOperationStatus::Running;
            task.progress = FileOperationProgress::pending();
            task.error = None;
            let _ = task.run_state_sender.send(FileOperationRunState::Running);
            return self.persist_task_state(position);
        }
        None
    }

    pub(super) fn restore_stored_task(&mut self, stored_task: StoredTask) -> Option<String> {
        let StoredTask {
            id,
            operation,
            status,
            progress,
            ..
        } = stored_task;

        if stored_status_is_terminal(status) {
            return None;
        }

        let Some(operation) = QueuedFileOperation::from_resumable_stored(operation) else {
            return self.mark_interrupted_non_resumable_task_failed(id, progress);
        };
        let runner_lease = match self
            .store
            .as_ref()
            .expect("restored queue has an operation store")
            .try_acquire_recoverable_task_runner(id)
        {
            Ok(Some(runner_lease)) => runner_lease,
            Ok(None) => return None,
            Err(error) => return Some(storage_error(error)),
        };

        let (task_status, initial_run_state, cancel_on_restore) = match status {
            StoredTaskStatus::Paused => (
                FileOperationStatus::Paused,
                FileOperationRunState::Paused,
                false,
            ),
            StoredTaskStatus::Canceling => (
                FileOperationStatus::Canceling,
                FileOperationRunState::Running,
                true,
            ),
            StoredTaskStatus::Pending
            | StoredTaskStatus::Running
            | StoredTaskStatus::RecoveryPending => (
                FileOperationStatus::Pending,
                FileOperationRunState::Running,
                false,
            ),
            StoredTaskStatus::Failed | StoredTaskStatus::Completed | StoredTaskStatus::Canceled => {
                return None
            }
        };
        let (run_state_sender, run_state_receiver) = watch::channel(initial_run_state);
        let cancel = CancellationToken::new();
        let is_read = self.is_panel_open;
        if cancel_on_restore {
            cancel.cancel();
        }
        self.tasks.push(FileOperationTask {
            id,
            operation,
            status: task_status,
            progress: FileOperationProgress::pending(),
            error: None,
            is_read,
            cancel,
            _runner_lease: Some(runner_lease),
            run_state_sender,
            run_state_receiver,
            is_persisted: true,
        });
        let position = self.tasks.len().saturating_sub(1);
        self.persist_task_state(position)
    }

    pub(super) fn mark_interrupted_non_resumable_task_failed(
        &self,
        id: u64,
        progress: StoredProgress,
    ) -> Option<String> {
        self.store
            .as_ref()?
            .update_task_state(
                id,
                StoredTaskStatus::Failed,
                progress,
                Some("Task was interrupted and cannot safely resume"),
            )
            .err()
            .map(storage_error)
    }

    pub(super) fn allocate_local_id(&mut self) -> u64 {
        loop {
            let id = self.next_local_id;
            self.next_local_id = id.checked_add(1).unwrap_or(LOCAL_TASK_ID_START);
            if !self.tasks.iter().any(|task| task.id == id) {
                return id;
            }
        }
    }

    pub(super) fn mark_all_read(&mut self) {
        for task in &mut self.tasks {
            task.is_read = true;
        }
    }

    pub(super) fn persist_task_status(&self, position: usize) -> Option<String> {
        let task = &self.tasks[position];
        if !task.is_persisted {
            return None;
        }
        self.store
            .as_ref()?
            .update_status(task.id, task.status.to_stored())
            .err()
            .map(storage_error)
    }

    pub(super) fn persist_task_state(&self, position: usize) -> Option<String> {
        self.persist_task_state_as(position, self.tasks[position].status.to_stored())
    }

    pub(super) fn persist_task_state_preserving_recovery(&self, position: usize) -> Option<String> {
        let store = self.store.as_ref()?;
        let task = &self.tasks[position];
        store
            .update_task_state(
                task.id,
                task.status.to_stored(),
                task.progress.to_stored(),
                task.error.as_deref(),
            )
            .err()
            .map(storage_error)
    }

    pub(super) fn persist_task_state_as(
        &self,
        position: usize,
        stored_status: StoredTaskStatus,
    ) -> Option<String> {
        let task = &self.tasks[position];
        if !task.is_persisted {
            return None;
        }
        let store = self.store.as_ref()?;
        let update = if task.operation.uses_recovery_journal()
            && matches!(
                stored_status,
                StoredTaskStatus::Completed | StoredTaskStatus::Canceled | StoredTaskStatus::Failed
            ) {
            store.finalize_recoverable_transfer_task(
                task.id,
                stored_status,
                task.progress.to_stored(),
                task.error.as_deref(),
            )
        } else {
            store.update_task_state(
                task.id,
                stored_status,
                task.progress.to_stored(),
                task.error.as_deref(),
            )
        };
        update.err().map(storage_error)
    }
}
