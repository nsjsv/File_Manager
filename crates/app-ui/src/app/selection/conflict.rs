use std::collections::HashSet;
use std::path::{Path, PathBuf};

use file_core::TransferConflictStrategy;
use iced::Task;

use crate::app::FileBrowser;
use crate::commands::{check_transfer_conflicts_command, check_transfer_rename_target_command};
use crate::model::{
    Message, TransferConflictChoice, TransferConflictItem, TransferConflictMode,
    TransferConflictState,
};
use crate::operation_queue::{QueuedFileOperation, QueuedTransfer};

impl FileBrowser {
    pub(in crate::app) fn enqueue_or_confirm_transfers(
        &mut self,
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
    ) -> Task<Message> {
        check_transfer_conflicts_command(mode, transfers)
    }

    pub(in crate::app) fn accept_transfer_conflicts_checked(
        &mut self,
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
        conflicts: Vec<TransferConflictItem>,
    ) -> Task<Message> {
        if conflicts.is_empty() {
            return self.enqueue_transfer_operation(mode, transfers);
        }

        let rename_input = conflict_default_name(&conflicts[0]);
        self.error = None;
        self.context_menu = None;
        self.operation_queue.close_panel();
        self.transfer_conflict = Some(TransferConflictState {
            mode,
            transfers,
            conflicts,
            current_index: 0,
            apply_to_all: false,
            rename_input,
        });
        Task::none()
    }

    fn enqueue_transfer_operation(
        &mut self,
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
    ) -> Task<Message> {
        match mode {
            TransferConflictMode::Copy => {
                self.enqueue_file_operation(QueuedFileOperation::Copy { transfers })
            }
            TransferConflictMode::Move => {
                self.enqueue_file_operation(QueuedFileOperation::Move { transfers })
            }
        }
    }

    pub(in crate::app) fn resolve_transfer_conflict_choice(
        &mut self,
        choice: TransferConflictChoice,
    ) -> Task<Message> {
        let Some(mut state) = self.transfer_conflict.take() else {
            return Task::none();
        };

        let apply_to_all = state.apply_to_all;
        loop {
            let Some(conflict) = state.current_conflict() else {
                break;
            };
            if choice == TransferConflictChoice::Merge && !conflict.can_merge() {
                if !apply_to_all {
                    self.error = Some("Only two folders can be merged".to_owned());
                }
                break;
            }

            apply_conflict_choice(&mut state, choice);
            state.current_index += 1;
            if !apply_to_all {
                break;
            }
        }

        self.finish_or_continue_transfer_conflicts(state)
    }

    pub(in crate::app) fn toggle_transfer_conflict_apply_to_all(&mut self) {
        if let Some(state) = &mut self.transfer_conflict {
            state.apply_to_all = !state.apply_to_all;
        }
    }

    pub(in crate::app) fn update_transfer_conflict_rename(&mut self, value: String) {
        if let Some(state) = &mut self.transfer_conflict {
            state.rename_input = value;
        }
    }

    pub(in crate::app) fn confirm_transfer_conflict_rename(&mut self) -> Task<Message> {
        let Some(state) = self.transfer_conflict.take() else {
            return Task::none();
        };

        let Some(conflict) = state.current_conflict() else {
            return self.finish_or_continue_transfer_conflicts(state);
        };
        let Some(parent) = conflict.target.parent().map(Path::to_path_buf) else {
            self.error = Some("Destination path has no parent directory".to_owned());
            self.transfer_conflict = Some(state);
            return Task::none();
        };

        let name = state.rename_input.trim();
        if !is_valid_transfer_rename(name) {
            self.error = Some("Enter a name without path separators".to_owned());
            self.transfer_conflict = Some(state);
            return Task::none();
        }

        let renamed_target = parent.join(name);
        let transfer_position = conflict_transfer_position(&state, conflict);
        let reserved_targets = reserved_targets_except(&state.transfers, transfer_position);
        if reserved_targets.contains(&renamed_target) {
            self.error = Some("That name already exists. Choose another name".to_owned());
            self.transfer_conflict = Some(state);
            return Task::none();
        }

        check_transfer_rename_target_command(state, transfer_position, renamed_target)
    }

    pub(in crate::app) fn accept_transfer_conflict_rename_target(
        &mut self,
        mut state: TransferConflictState,
        transfer_position: Option<usize>,
        renamed_target: PathBuf,
        available: Result<bool, String>,
    ) -> Task<Message> {
        match available {
            Ok(true) => {}
            Ok(false) => {
                self.error = Some("That name already exists. Choose another name".to_owned());
                self.transfer_conflict = Some(state);
                return Task::none();
            }
            Err(error) => {
                self.error = Some(error);
                self.transfer_conflict = Some(state);
                return Task::none();
            }
        }

        if let Some(position) = transfer_position {
            state.transfers[position].target = renamed_target;
            state.transfers[position].conflict_strategy = TransferConflictStrategy::Fail;
        }
        state.current_index += 1;
        self.error = None;
        self.finish_or_continue_transfer_conflicts(state)
    }

    fn finish_or_continue_transfer_conflicts(
        &mut self,
        mut state: TransferConflictState,
    ) -> Task<Message> {
        if state.current_index >= state.conflicts.len() {
            self.transfer_conflict = None;
            return self.enqueue_transfer_operation(state.mode, state.transfers);
        }

        if let Some(conflict) = state.current_conflict() {
            state.rename_input = conflict_default_name(conflict);
        }
        self.transfer_conflict = Some(state);
        Task::none()
    }
}
fn apply_conflict_choice(state: &mut TransferConflictState, choice: TransferConflictChoice) {
    let Some(conflict) = state.current_conflict().cloned() else {
        return;
    };
    let Some(position) = conflict_transfer_position(state, &conflict) else {
        return;
    };

    let strategy = match choice {
        TransferConflictChoice::Replace => TransferConflictStrategy::Replace,
        TransferConflictChoice::Skip => TransferConflictStrategy::Skip,
        TransferConflictChoice::KeepBoth => TransferConflictStrategy::KeepBoth,
        TransferConflictChoice::Merge => TransferConflictStrategy::Merge,
    };
    state.transfers[position].conflict_strategy = strategy;
}

fn conflict_transfer_position(
    state: &TransferConflictState,
    conflict: &TransferConflictItem,
) -> Option<usize> {
    state.transfers.iter().position(|transfer| {
        transfer.source == conflict.source && transfer.target == conflict.target
    })
}

fn reserved_targets_except(
    transfers: &[QueuedTransfer],
    excluded_position: Option<usize>,
) -> HashSet<PathBuf> {
    transfers
        .iter()
        .enumerate()
        .filter(|(position, _)| Some(*position) != excluded_position)
        .map(|(_, transfer)| transfer.target.clone())
        .collect()
}

fn conflict_default_name(conflict: &TransferConflictItem) -> String {
    conflict
        .source
        .file_name()
        .or_else(|| conflict.target.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "item".to_owned())
}

fn is_valid_transfer_rename(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\')
}
