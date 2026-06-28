use file_core::TransferConflictStrategy;
use iced::Task;

use crate::app::FileBrowser;
use crate::commands::check_transfer_conflicts_command;
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

        self.error = None;
        self.context_menu = None;
        self.operation_queue.close_panel();
        self.transfer_conflict = Some(TransferConflictState {
            mode,
            transfers,
            conflicts,
            current_index: 0,
            apply_to_all: false,
        });
        self.schedule_transfer_conflict_thumbnails()
    }

    fn enqueue_transfer_operation(
        &mut self,
        mode: TransferConflictMode,
        transfers: Vec<QueuedTransfer>,
    ) -> Task<Message> {
        match mode {
            TransferConflictMode::Copy => self.enqueue_file_operation(QueuedFileOperation::Copy {
                transfers,
                verification: self.file_operation_verification(),
            }),
            TransferConflictMode::Move => self.enqueue_file_operation(QueuedFileOperation::Move {
                transfers,
                verification: self.file_operation_verification(),
            }),
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
            if state.current_conflict().is_none() {
                break;
            };
            apply_conflict_choice(&mut state, choice);
            state.current_index += 1;
            if !apply_to_all {
                break;
            }
        }

        self.finish_or_continue_transfer_conflicts(state)
    }

    pub(in crate::app) fn apply_transfer_conflict_message(
        &mut self,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::TransferConflictChoiceSelected(choice) => {
                self.resolve_transfer_conflict_choice(choice)
            }
            Message::TransferConflictApplyToAllToggled => {
                self.toggle_transfer_conflict_apply_to_all();
                Task::none()
            }
            Message::TransferConflictCancelRequested => {
                self.transfer_conflict = None;
                Task::none()
            }
            _ => Task::none(),
        }
    }

    pub(in crate::app) fn toggle_transfer_conflict_apply_to_all(&mut self) {
        if let Some(state) = &mut self.transfer_conflict {
            state.apply_to_all = !state.apply_to_all;
        }
    }

    fn finish_or_continue_transfer_conflicts(
        &mut self,
        state: TransferConflictState,
    ) -> Task<Message> {
        if state.current_index >= state.conflicts.len() {
            self.transfer_conflict = None;
            return self.enqueue_transfer_operation(state.mode, state.transfers);
        }

        self.transfer_conflict = Some(state);
        self.schedule_transfer_conflict_thumbnails()
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
        TransferConflictChoice::Rename => TransferConflictStrategy::KeepBoth,
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use file_core::{TransferConflictMetadata, TransferConflictStrategy};

    use super::*;

    fn test_conflict_state() -> TransferConflictState {
        let source = PathBuf::from("/source/report.txt");
        let target = PathBuf::from("/target/report.txt");

        TransferConflictState {
            mode: TransferConflictMode::Copy,
            transfers: vec![QueuedTransfer::new(source.clone(), target.clone())],
            conflicts: vec![TransferConflictItem {
                source,
                target,
                source_metadata: TransferConflictMetadata {
                    is_directory: false,
                    len: 3,
                    modified: None,
                },
                target_metadata: TransferConflictMetadata {
                    is_directory: false,
                    len: 3,
                    modified: None,
                },
            }],
            current_index: 0,
            apply_to_all: false,
        }
    }

    #[test]
    fn transfer_conflict_rename_uses_keep_both_strategy() {
        let mut state = test_conflict_state();

        apply_conflict_choice(&mut state, TransferConflictChoice::Rename);

        assert_eq!(
            state.transfers[0].conflict_strategy,
            TransferConflictStrategy::KeepBoth
        );
    }
}
