use std::path::Path;

use tokio::fs;

use super::{
    commit_artifact, commit_payload_path, inspect_optional_identity, manifest_root_identity,
    next_recovered_path, path_exists, persist_checkpoint, record_manifest,
    record_replacement_manifest, sync_rename_parents, target_parent,
};
use crate::ops::recoverable_transfer::{
    fingerprint_object, inspect_file_identity, plan_owned_artifact, recover_owned_artifact,
    remove_empty_owned_artifact, rename_noreplace, validate_owned_artifact, verify_source_manifest,
    CommitPayload, CommitTransfer, CommittedTransfer, CompletedTarget, FileIdentity,
    FileObjectKind, NoReplaceRenameError, OwnedArtifact, OwnedArtifactKind,
    OwnedTreeEntryDeletionIntent, PreparedTransfer, RecoverableTransferError,
    RecoverableTransferOperation, RetiredSource, SourceDisposition, SourceManifestEntry,
    SourceRetirementPlan, StagedSourceLocation, TransferCheckpoint, TransferExecutionKind,
    TransferJournal, TransferJournalRecord,
};
use crate::transfer_conflict::available_transfer_target_path_candidate;

pub(super) async fn commit_transfer<J: TransferJournal>(
    record: &mut TransferJournalRecord,
    journal: &J,
    mut commit: CommitTransfer,
) -> Result<(), RecoverableTransferError> {
    let payload_path = commit_payload_path(record, &commit);
    let payload_exists = path_exists(&payload_path).await?;
    let target_exists = path_exists(&commit.prepared.resolved_target).await?;

    if target_exists && !payload_exists {
        return persist_inferred_commit(record, journal, commit).await;
    }
    if !payload_exists {
        return Err(RecoverableTransferError::SourceChanged { path: payload_path });
    }
    let payload_identity = inspect_file_identity(&payload_path).await?;
    if payload_identity != *commit_payload_identity(&commit)
        || fingerprint_object(&payload_path).await? != commit.fingerprint
    {
        return Err(RecoverableTransferError::FingerprintMismatch { path: payload_path });
    }

    if let Some(expected_target_identity) = &commit.prepared.expected_target_identity {
        prepare_replace_backup(&commit, expected_target_identity).await?;
    } else if target_exists {
        if record.request.conflict_strategy == crate::TransferConflictStrategy::KeepBoth {
            commit.prepared.resolved_target =
                available_transfer_target_path_candidate(&record.request.requested_target)
                    .await
                    .map_err(|source| {
                        RecoverableTransferError::file_system(
                            "select keep-both target for",
                            &record.request.requested_target,
                            source,
                        )
                    })?;
            persist_checkpoint(record, journal, TransferCheckpoint::CommitIntent(commit)).await?;
            return Ok(());
        }
        return Err(RecoverableTransferError::TargetConflict {
            path: commit.prepared.resolved_target,
        });
    }

    match rename_noreplace(&payload_path, &commit.prepared.resolved_target) {
        Ok(()) => {}
        Err(NoReplaceRenameError::CrossDevice)
            if matches!(commit.payload, CommitPayload::DirectSource { .. }) =>
        {
            let mut prepared = commit.prepared;
            prepared.execution = TransferExecutionKind::MoveToStage;
            prepared.staging_plan = Some(plan_owned_artifact(
                target_parent(&prepared.resolved_target)?,
                OwnedArtifactKind::TargetStaging,
                record.owner(0),
            )?);
            persist_checkpoint(
                record,
                journal,
                TransferCheckpoint::StageCreationIntent(prepared),
            )
            .await?;
            return Ok(());
        }
        Err(NoReplaceRenameError::TargetExists)
            if record.request.conflict_strategy == crate::TransferConflictStrategy::KeepBoth =>
        {
            commit.prepared.resolved_target =
                available_transfer_target_path_candidate(&record.request.requested_target)
                    .await
                    .map_err(|source| {
                        RecoverableTransferError::file_system(
                            "select keep-both target for",
                            &record.request.requested_target,
                            source,
                        )
                    })?;
            persist_checkpoint(record, journal, TransferCheckpoint::CommitIntent(commit)).await?;
            return Ok(());
        }
        Err(error) => {
            return Err(error.into_transfer_error(&payload_path, &commit.prepared.resolved_target));
        }
    }
    sync_rename_parents(&payload_path, &commit.prepared.resolved_target).await?;
    let committed = committed_transfer(record, &commit).await?;
    persist_checkpoint(
        record,
        journal,
        TransferCheckpoint::TargetCommitted(committed),
    )
    .await
}

async fn prepare_replace_backup(
    commit: &CommitTransfer,
    expected_target_identity: &FileIdentity,
) -> Result<(), RecoverableTransferError> {
    let artifact = commit_artifact(commit)?;
    let backup_path = artifact.plan.backup_path();
    if path_exists(&backup_path).await? {
        verify_expected_backup(commit, expected_target_identity, &backup_path).await?;
        return Ok(());
    }

    let current_target = inspect_optional_identity(&commit.prepared.resolved_target)
        .await?
        .ok_or_else(|| RecoverableTransferError::TargetConflict {
            path: commit.prepared.resolved_target.clone(),
        })?;
    let current_fingerprint = fingerprint_object(&commit.prepared.resolved_target).await?;
    if current_target != *expected_target_identity
        || Some(current_fingerprint) != commit.prepared.expected_target_fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict {
            path: commit.prepared.resolved_target.clone(),
        });
    }
    rename_noreplace(&commit.prepared.resolved_target, &backup_path).map_err(|error| {
        error.into_transfer_error(&commit.prepared.resolved_target, &backup_path)
    })?;
    sync_rename_parents(&commit.prepared.resolved_target, &backup_path).await?;
    verify_expected_backup(commit, expected_target_identity, &backup_path).await
}

async fn verify_expected_backup(
    commit: &CommitTransfer,
    expected_target_identity: &FileIdentity,
    backup_path: &Path,
) -> Result<(), RecoverableTransferError> {
    let backup_identity = inspect_file_identity(backup_path).await?;
    let backup_fingerprint = fingerprint_object(backup_path).await?;
    if !backup_identity.same_object(expected_target_identity)
        || Some(backup_fingerprint) != commit.prepared.expected_target_fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict {
            path: commit.prepared.resolved_target.clone(),
        });
    }
    Ok(())
}

pub(super) fn commit_payload_identity(commit: &CommitTransfer) -> &FileIdentity {
    match &commit.payload {
        CommitPayload::DirectSource { identity } => identity,
        CommitPayload::Artifact {
            payload_identity, ..
        } => payload_identity,
    }
}

async fn persist_inferred_commit<J: TransferJournal>(
    record: &mut TransferJournalRecord,
    journal: &J,
    commit: CommitTransfer,
) -> Result<(), RecoverableTransferError> {
    let target_identity = inspect_file_identity(&commit.prepared.resolved_target).await?;
    let target_fingerprint = fingerprint_object(&commit.prepared.resolved_target).await?;
    if !target_identity.same_object(commit_payload_identity(&commit))
        || target_fingerprint != commit.fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict {
            path: commit.prepared.resolved_target,
        });
    }
    let committed = committed_transfer(record, &commit).await?;
    persist_checkpoint(
        record,
        journal,
        TransferCheckpoint::TargetCommitted(committed),
    )
    .await
}

async fn committed_transfer(
    record: &TransferJournalRecord,
    commit: &CommitTransfer,
) -> Result<CommittedTransfer, RecoverableTransferError> {
    let target_identity = inspect_file_identity(&commit.prepared.resolved_target).await?;
    let target_fingerprint = fingerprint_object(&commit.prepared.resolved_target).await?;
    if !target_identity.same_object(commit_payload_identity(commit))
        || target_fingerprint != commit.fingerprint
    {
        return Err(RecoverableTransferError::FingerprintMismatch {
            path: commit.prepared.resolved_target.clone(),
        });
    }
    let (artifact, staged_source_location) = match &commit.payload {
        CommitPayload::DirectSource { .. } => (None, None),
        CommitPayload::Artifact {
            artifact,
            source_location,
            ..
        } => (Some(artifact.clone()), Some(*source_location)),
    };
    let source_disposition = match record.request.operation {
        RecoverableTransferOperation::Copy => SourceDisposition::Preserved,
        RecoverableTransferOperation::Move
            if staged_source_location == Some(StagedSourceLocation::OriginalPath) =>
        {
            SourceDisposition::RequiresRetirement
        }
        RecoverableTransferOperation::Move => SourceDisposition::MovedByCommit,
    };
    Ok(CommittedTransfer {
        final_target: commit.prepared.resolved_target.clone(),
        target_identity,
        fingerprint: commit.fingerprint,
        artifact,
        source_disposition,
        backup_fingerprint: commit.prepared.expected_target_fingerprint,
        backup_cleanup_index: 0,
        backup_cleanup_intent: None,
    })
}

pub(super) async fn advance_committed_transfer<J: TransferJournal>(
    record: &mut TransferJournalRecord,
    journal: &J,
    mut committed: CommittedTransfer,
) -> Result<(), RecoverableTransferError> {
    if let Err(target_error) = verify_committed_target(&committed).await {
        if let Some(recovered_backup) =
            recover_complete_backup_after_target_conflict(record, &committed).await?
        {
            return Err(RecoverableTransferError::RecoveryBlocked {
                diagnostic: format!(
                    "committed target verification failed: {target_error}; previous target recovered to {}",
                    recovered_backup.display()
                ),
            });
        }
        return Err(target_error);
    }
    if let Some(artifact) = committed.artifact.clone() {
        if let Some(backup_fingerprint) = committed.backup_fingerprint {
            let manifest_entries = &record_replacement_manifest(record)?.entries;
            if committed.backup_cleanup_index > manifest_entries.len() {
                return Err(RecoverableTransferError::InvalidCheckpoint {
                    message: "replacement backup cleanup cursor exceeds the manifest".to_owned(),
                });
            }
            let current_entry =
                reverse_manifest_entry(manifest_entries, committed.backup_cleanup_index);
            if let Some(intent) = committed.backup_cleanup_intent.clone() {
                let expected = current_entry.as_ref().ok_or_else(|| {
                    RecoverableTransferError::InvalidCheckpoint {
                        message: "replacement backup cleanup intent has no manifest entry"
                            .to_owned(),
                    }
                })?;
                if &intent.entry != expected {
                    return Err(RecoverableTransferError::InvalidCheckpoint {
                        message: "replacement backup cleanup intent does not match the manifest"
                            .to_owned(),
                    });
                }
                remove_owned_tree_entry(
                    &artifact,
                    &artifact.plan.backup_path(),
                    &artifact.plan.payload_path(),
                    &committed.final_target,
                    &intent,
                )
                .await?;
                committed.backup_cleanup_index += 1;
                committed.backup_cleanup_intent = None;
                return persist_checkpoint(
                    record,
                    journal,
                    TransferCheckpoint::TargetCommitted(committed),
                )
                .await;
            }
            if let Some(entry) = current_entry.as_ref() {
                committed.backup_cleanup_intent = Some(
                    prepare_owned_tree_entry_deletion_intent(
                        &artifact,
                        &artifact.plan.backup_path(),
                        entry,
                        backup_fingerprint,
                    )
                    .await?,
                );
                return persist_checkpoint(
                    record,
                    journal,
                    TransferCheckpoint::TargetCommitted(committed),
                )
                .await;
            }
        } else if record.replacement_manifest.is_some() {
            return Err(RecoverableTransferError::InvalidCheckpoint {
                message: "replacement manifest has no backup fingerprint".to_owned(),
            });
        }
        if path_exists(&artifact.plan.root).await? {
            remove_empty_owned_artifact(&artifact).await?;
        }
        committed.artifact = None;
    }
    match committed.source_disposition {
        SourceDisposition::RequiresRetirement => {
            let source_parent = target_parent(&record.request.source)?;
            let artifact_plan = plan_owned_artifact(
                source_parent,
                OwnedArtifactKind::SourceRetirement,
                record.owner(1),
            )?;
            persist_checkpoint(
                record,
                journal,
                TransferCheckpoint::SourceRetirementIntent(SourceRetirementPlan {
                    committed,
                    artifact_plan,
                    artifact: None,
                }),
            )
            .await
        }
        SourceDisposition::Preserved | SourceDisposition::MovedByCommit => {
            let completed = completed_target(&committed);
            persist_checkpoint(record, journal, TransferCheckpoint::Completed(completed)).await
        }
    }
}

async fn recover_complete_backup_after_target_conflict(
    record: &TransferJournalRecord,
    committed: &CommittedTransfer,
) -> Result<Option<std::path::PathBuf>, RecoverableTransferError> {
    let (Some(artifact), Some(expected_fingerprint)) =
        (&committed.artifact, committed.backup_fingerprint)
    else {
        return Ok(None);
    };
    let backup_path = artifact.plan.backup_path();
    if !path_exists(&backup_path).await? {
        return Ok(None);
    }
    validate_owned_artifact(artifact).await?;
    let expected_identity = manifest_root_identity(record_replacement_manifest(record)?)?;
    let backup_identity = inspect_file_identity(&backup_path).await?;
    let backup_fingerprint = fingerprint_object(&backup_path).await?;
    if !backup_identity.same_object(expected_identity) || backup_fingerprint != expected_fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict { path: backup_path });
    }

    let recovered_path = next_recovered_path(&committed.final_target).await?;
    rename_noreplace(&backup_path, &recovered_path)
        .map_err(|error| error.into_transfer_error(&backup_path, &recovered_path))?;
    sync_rename_parents(&backup_path, &recovered_path).await?;
    Ok(Some(recovered_path))
}

pub(super) async fn retire_source<J: TransferJournal>(
    record: &mut TransferJournalRecord,
    journal: &J,
    retirement: SourceRetirementPlan,
) -> Result<(), RecoverableTransferError> {
    verify_committed_target(&retirement.committed).await?;
    let Some(artifact) = retirement.artifact.clone() else {
        let artifact = recover_owned_artifact(retirement.artifact_plan.clone()).await?;
        let mut identified_retirement = retirement;
        identified_retirement.artifact = Some(artifact);
        return persist_checkpoint(
            record,
            journal,
            TransferCheckpoint::SourceRetirementIntent(identified_retirement),
        )
        .await;
    };
    crate::ops::recoverable_transfer::validate_owned_artifact(&artifact).await?;
    let prepared = PreparedTransfer {
        source_identity: manifest_root_identity(record_manifest(record)?)?.clone(),
        resolved_target: retirement.committed.final_target.clone(),
        expected_target_identity: None,
        expected_target_fingerprint: None,
        source_fingerprint: retirement.committed.fingerprint,
        execution: TransferExecutionKind::MoveToStage,
        staging_plan: None,
    };
    let payload_path = artifact.plan.payload_path();
    let source_exists = path_exists(&record.request.source).await?;
    let payload_exists = path_exists(&payload_path).await?;
    match (source_exists, payload_exists) {
        (true, false) => {
            verify_prepared_source(record, &prepared).await?;
            rename_noreplace(&record.request.source, &payload_path).map_err(|error| {
                error.into_transfer_error(&record.request.source, &payload_path)
            })?;
            sync_rename_parents(&record.request.source, &payload_path).await?;
        }
        (false, true) => {}
        _ => {
            return Err(RecoverableTransferError::InvalidCheckpoint {
                message: "source retirement paths are ambiguous".to_owned(),
            });
        }
    }
    let payload_identity = inspect_file_identity(&payload_path).await?;
    if !payload_identity.same_object(&prepared.source_identity)
        || fingerprint_object(&payload_path).await? != retirement.committed.fingerprint
    {
        return Err(RecoverableTransferError::SourceChanged { path: payload_path });
    }
    persist_checkpoint(
        record,
        journal,
        TransferCheckpoint::SourceRetired(RetiredSource {
            committed: retirement.committed,
            artifact,
            cleanup_index: 0,
            cleanup_intent: None,
        }),
    )
    .await
}

pub(super) async fn advance_retired_source<J: TransferJournal>(
    record: &mut TransferJournalRecord,
    journal: &J,
    mut retired: RetiredSource,
) -> Result<(), RecoverableTransferError> {
    verify_committed_target(&retired.committed).await?;
    let manifest_entries = &record_manifest(record)?.entries;
    if retired.cleanup_index > manifest_entries.len() {
        return Err(RecoverableTransferError::InvalidCheckpoint {
            message: "source retirement cleanup cursor exceeds the manifest".to_owned(),
        });
    }
    let current_entry = reverse_manifest_entry(manifest_entries, retired.cleanup_index);

    if let Some(intent) = retired.cleanup_intent.clone() {
        let expected =
            current_entry
                .as_ref()
                .ok_or_else(|| RecoverableTransferError::InvalidCheckpoint {
                    message: "source retirement cleanup intent has no manifest entry".to_owned(),
                })?;
        if &intent.entry != expected {
            return Err(RecoverableTransferError::InvalidCheckpoint {
                message: "source retirement cleanup intent does not match the manifest".to_owned(),
            });
        }
        remove_owned_tree_entry(
            &retired.artifact,
            &retired.artifact.plan.payload_path(),
            &retired.artifact.plan.backup_path(),
            &record.request.source,
            &intent,
        )
        .await?;
        retired.cleanup_index += 1;
        retired.cleanup_intent = None;
        return persist_checkpoint(record, journal, TransferCheckpoint::SourceRetired(retired))
            .await;
    }

    if let Some(entry) = current_entry.as_ref() {
        retired.cleanup_intent = Some(
            prepare_owned_tree_entry_deletion_intent(
                &retired.artifact,
                &retired.artifact.plan.payload_path(),
                entry,
                retired.committed.fingerprint,
            )
            .await?,
        );
        return persist_checkpoint(record, journal, TransferCheckpoint::SourceRetired(retired))
            .await;
    }

    remove_empty_owned_artifact(&retired.artifact).await?;
    let completed = completed_target(&retired.committed);
    persist_checkpoint(record, journal, TransferCheckpoint::Completed(completed)).await
}

fn reverse_manifest_entry(
    entries: &[SourceManifestEntry],
    completed_entries: usize,
) -> Option<SourceManifestEntry> {
    completed_entries
        .checked_add(1)
        .and_then(|offset| entries.len().checked_sub(offset))
        .and_then(|index| entries.get(index))
        .cloned()
}

async fn prepare_owned_tree_entry_deletion_intent(
    artifact: &OwnedArtifact,
    tree_root: &Path,
    entry: &SourceManifestEntry,
    root_fingerprint: crate::ops::recoverable_transfer::ObjectFingerprint,
) -> Result<OwnedTreeEntryDeletionIntent, RecoverableTransferError> {
    crate::ops::recoverable_transfer::validate_owned_artifact(artifact).await?;
    let entry_path = owned_tree_entry_path(tree_root, entry);
    let entry_position = if entry.relative_path.as_os_str().is_empty() {
        ManifestEntryPosition::Root
    } else {
        ManifestEntryPosition::Descendant
    };
    let fingerprint = if path_exists(&entry_path).await? {
        verify_retirement_entry_before_intent(
            &entry_path,
            &entry.identity,
            entry_position,
            root_fingerprint,
        )
        .await?;
        Some(fingerprint_object(&entry_path).await?)
    } else {
        None
    };
    Ok(OwnedTreeEntryDeletionIntent {
        entry: entry.clone(),
        fingerprint,
    })
}

async fn remove_owned_tree_entry(
    artifact: &OwnedArtifact,
    tree_root: &Path,
    deletion_slot: &Path,
    recovered_base: &Path,
    intent: &OwnedTreeEntryDeletionIntent,
) -> Result<(), RecoverableTransferError> {
    crate::ops::recoverable_transfer::validate_owned_artifact(artifact).await?;
    let entry_path = owned_tree_entry_path(tree_root, &intent.entry);
    let entry_exists = path_exists(&entry_path).await?;
    let slot_exists = path_exists(deletion_slot).await?;

    match (entry_exists, slot_exists) {
        (true, false) => {
            let expected_fingerprint =
                intent
                    .fingerprint
                    .ok_or_else(|| RecoverableTransferError::SourceChanged {
                        path: entry_path.clone(),
                    })?;
            verify_retirement_entry_after_intent(
                &entry_path,
                &intent.entry.identity,
                expected_fingerprint,
            )
            .await?;
            rename_noreplace(&entry_path, deletion_slot)
                .map_err(|error| error.into_transfer_error(&entry_path, deletion_slot))?;
            sync_rename_parents(&entry_path, deletion_slot).await?;
        }
        (false, true) | (false, false) => {}
        (true, true) => {
            return Err(RecoverableTransferError::InvalidCheckpoint {
                message: "source retirement entry and deletion slot both exist".to_owned(),
            });
        }
    }

    if path_exists(deletion_slot).await? {
        let Some(expected_fingerprint) = intent.fingerprint else {
            return Err(RecoverableTransferError::SourceChanged {
                path: deletion_slot.to_path_buf(),
            });
        };
        if let Err(error) = verify_retirement_entry_after_intent(
            deletion_slot,
            &intent.entry.identity,
            expected_fingerprint,
        )
        .await
        {
            let slot_identity = inspect_file_identity(deletion_slot).await?;
            if slot_identity.same_object(&intent.entry.identity) {
                restore_unverified_owned_tree_entry(&entry_path, deletion_slot, recovered_base)
                    .await?;
            }
            return Err(error);
        }
        remove_retirement_slot(deletion_slot).await?;
        sync_removed_entry_parent(deletion_slot).await?;
    }
    Ok(())
}

fn owned_tree_entry_path(tree_root: &Path, entry: &SourceManifestEntry) -> std::path::PathBuf {
    if entry.relative_path.as_os_str().is_empty() {
        tree_root.to_path_buf()
    } else {
        tree_root.join(&entry.relative_path)
    }
}

#[derive(Clone, Copy)]
enum ManifestEntryPosition {
    Root,
    Descendant,
}

async fn verify_retirement_entry_before_intent(
    path: &Path,
    expected: &FileIdentity,
    position: ManifestEntryPosition,
    root_fingerprint: crate::ops::recoverable_transfer::ObjectFingerprint,
) -> Result<(), RecoverableTransferError> {
    let current = inspect_file_identity(path).await?;
    let matches = match expected.object_kind {
        FileObjectKind::Directory => {
            if current.object_kind != FileObjectKind::Directory || !current.same_object(expected) {
                false
            } else {
                let mut entries = fs::read_dir(path).await.map_err(|source| {
                    RecoverableTransferError::file_system(
                        "read retiring source directory",
                        path,
                        source,
                    )
                })?;
                entries
                    .next_entry()
                    .await
                    .map_err(|source| {
                        RecoverableTransferError::file_system(
                            "read retiring source directory entry",
                            path,
                            source,
                        )
                    })?
                    .is_none()
            }
        }
        FileObjectKind::RegularFile | FileObjectKind::SymbolicLink
            if matches!(position, ManifestEntryPosition::Root) =>
        {
            current.same_object(expected) && fingerprint_object(path).await? == root_fingerprint
        }
        FileObjectKind::RegularFile | FileObjectKind::SymbolicLink => current == *expected,
    };
    if matches {
        Ok(())
    } else {
        Err(RecoverableTransferError::SourceChanged {
            path: path.to_path_buf(),
        })
    }
}

async fn verify_retirement_entry_after_intent(
    path: &Path,
    expected: &FileIdentity,
    expected_fingerprint: crate::ops::recoverable_transfer::ObjectFingerprint,
) -> Result<(), RecoverableTransferError> {
    let current = inspect_file_identity(path).await?;
    if current.same_object(expected) && fingerprint_object(path).await? == expected_fingerprint {
        Ok(())
    } else {
        Err(RecoverableTransferError::SourceChanged {
            path: path.to_path_buf(),
        })
    }
}

async fn restore_unverified_owned_tree_entry(
    original_artifact_path: &Path,
    deletion_slot: &Path,
    recovered_base: &Path,
) -> Result<(), RecoverableTransferError> {
    let restored = if !path_exists(original_artifact_path).await? {
        original_artifact_path.to_path_buf()
    } else {
        super::next_recovered_path(recovered_base).await?
    };
    rename_noreplace(deletion_slot, &restored)
        .map_err(|error| error.into_transfer_error(deletion_slot, &restored))?;
    sync_rename_parents(deletion_slot, &restored).await
}

async fn remove_retirement_slot(path: &Path) -> Result<(), RecoverableTransferError> {
    let metadata = fs::symlink_metadata(path).await.map_err(|source| {
        RecoverableTransferError::file_system("read source deletion slot", path, source)
    })?;
    let removal = if metadata.file_type().is_dir() {
        fs::remove_dir(path).await
    } else {
        fs::remove_file(path).await
    };
    removal.map_err(|source| {
        RecoverableTransferError::file_system("remove verified source entry", path, source)
    })
}

async fn sync_removed_entry_parent(path: &Path) -> Result<(), RecoverableTransferError> {
    let path = path.to_path_buf();
    let error_path = path.clone();
    tokio::task::spawn_blocking(move || {
        crate::ops::recoverable_transfer::sync_parent_blocking(&path)
    })
    .await
    .map_err(|join_error| {
        RecoverableTransferError::file_system(
            "join source deletion sync task for",
            &error_path,
            std::io::Error::other(join_error),
        )
    })?
}

pub(super) fn completed_target(committed: &CommittedTransfer) -> CompletedTarget {
    CompletedTarget {
        path: committed.final_target.clone(),
        identity: committed.target_identity.clone(),
        fingerprint: committed.fingerprint,
    }
}

pub(super) async fn verify_completed_target(
    completed: &CompletedTarget,
) -> Result<(), RecoverableTransferError> {
    let current_identity = inspect_file_identity(&completed.path).await?;
    if current_identity == completed.identity {
        return Ok(());
    }
    if !current_identity.same_object(&completed.identity)
        || fingerprint_object(&completed.path).await? != completed.fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict {
            path: completed.path.clone(),
        });
    }
    Ok(())
}

pub(super) async fn verify_committed_target(
    committed: &CommittedTransfer,
) -> Result<(), RecoverableTransferError> {
    let current_identity = inspect_file_identity(&committed.final_target).await?;
    if current_identity == committed.target_identity {
        return Ok(());
    }
    if !current_identity.same_object(&committed.target_identity)
        || fingerprint_object(&committed.final_target).await? != committed.fingerprint
    {
        return Err(RecoverableTransferError::TargetConflict {
            path: committed.final_target.clone(),
        });
    }
    Ok(())
}

pub(super) async fn verify_prepared_source(
    record: &TransferJournalRecord,
    prepared: &PreparedTransfer,
) -> Result<(), RecoverableTransferError> {
    let manifest = record_manifest(record)?;
    verify_source_manifest(manifest).await?;
    let source_identity = inspect_file_identity(&record.request.source).await?;
    if source_identity != prepared.source_identity
        || fingerprint_object(&record.request.source).await? != prepared.source_fingerprint
    {
        return Err(RecoverableTransferError::SourceChanged {
            path: record.request.source.clone(),
        });
    }
    Ok(())
}
