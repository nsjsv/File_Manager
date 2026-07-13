use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::database::{
    DirectorySnapshot, EntryObservationState, KnownFileEntry, ObservedFile, ScanFileMutation,
    MAX_CLASSIFICATION_BATCH_ENTRIES, MAX_KNOWN_ENTRY_PAGE_ENTRIES,
};
use crate::error::{SearchError, SearchResult};
use crate::filesystem::{
    ensure_not_cancelled, observe_path, FilesystemEntry, FilesystemObservation,
    LocalFilesystemBoundary, TraversalDepth, TraversalEvent,
};
use crate::model::SearchFileKind;
use crate::writer::{scan_file_mutation_bytes, MAX_WRITER_FILE_PAYLOAD_BYTES};

use super::{
    collapse_affected_prefixes, directory_signature, extraction_plan_reads_content,
    observed_file_signature, path_is_same_or_descendant, push_pending_entry,
    report_checking_if_needed, report_crawling, report_crawling_if_needed,
    ChangedFilePipelineOutcome, IndexMaintenanceProgress, RebuildStats, SearchIndexer,
};

impl SearchIndexer {
    pub(super) async fn startup_check_scopes(
        &self,
        scopes: Vec<PathBuf>,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        for scope in collapse_affected_prefixes(scopes) {
            ensure_not_cancelled(cancellation)?;
            if self.writer.directory_snapshot(scope.clone())?.is_some() {
                self.warm_check_scope(&scope, stats, cancellation, on_progress)
                    .await?;
            } else {
                self.cold_scan_scope(&scope, stats, cancellation, on_progress)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn warm_check_scope(
        &self,
        scope: &Path,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        on_progress(IndexMaintenanceProgress::Checking {
            checked_entries: stats.checked,
            changed_entries: stats.changed,
        });

        let mut after_directory = None;
        loop {
            ensure_not_cancelled(cancellation)?;
            let page = self.writer.directory_snapshots_page(
                scope.to_path_buf(),
                after_directory.clone(),
                MAX_KNOWN_ENTRY_PAGE_ENTRIES,
            )?;
            if page.is_empty() {
                break;
            }
            after_directory = page.last().map(|snapshot| snapshot.path.clone());
            let page_snapshot_epoch = stats.directory_snapshots_changed;
            for snapshot in page {
                ensure_not_cancelled(cancellation)?;
                let snapshot = if stats.directory_snapshots_changed == page_snapshot_epoch {
                    snapshot
                } else {
                    // 父目录核对可能删除本页后续子树；继续使用页内旧快照会误删刚替换的新条目。
                    let Some(current_snapshot) =
                        self.writer.directory_snapshot(snapshot.path.clone())?
                    else {
                        continue;
                    };
                    current_snapshot
                };
                stats.checked += 1;
                match self.observe_index_path(&snapshot.path)? {
                    FilesystemObservation::Complete(entry)
                        if entry.kind() == SearchFileKind::Directory =>
                    {
                        let signature = directory_signature(entry.metadata());
                        if snapshot.observation_state == EntryObservationState::Observable
                            && snapshot.signature == signature
                        {
                            report_checking_if_needed(stats, on_progress);
                            continue;
                        }
                        stats.changed += 1;
                        self.reconcile_directory(&snapshot.path, stats, cancellation, on_progress)
                            .await?;
                    }
                    FilesystemObservation::Complete(_) => {
                        self.delete_scope(&snapshot.path, true, stats, on_progress)?;
                    }
                    FilesystemObservation::Inaccessible { scope } => {
                        self.mark_scope_inaccessible(&scope, true, stats, on_progress)?;
                    }
                    FilesystemObservation::Missing { scope }
                    | FilesystemObservation::PolicyExcluded { scope } => {
                        self.delete_scope(&scope, true, stats, on_progress)?;
                    }
                }
                report_checking_if_needed(stats, on_progress);
            }
        }

        let mut after_file = None;
        loop {
            ensure_not_cancelled(cancellation)?;
            let page = self.writer.known_files_page(
                scope.to_path_buf(),
                after_file.clone(),
                MAX_KNOWN_ENTRY_PAGE_ENTRIES,
            )?;
            if page.is_empty() {
                break;
            }
            after_file = page.last().map(|entry| entry.path.clone());
            for known_file in page {
                ensure_not_cancelled(cancellation)?;
                self.check_known_file(known_file, stats, cancellation, on_progress)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn reconcile_changed_paths(
        &self,
        changed_paths: Vec<PathBuf>,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        on_progress(IndexMaintenanceProgress::Checking {
            checked_entries: stats.checked,
            changed_entries: stats.changed,
        });
        ensure_not_cancelled(cancellation)?;
        let changed_paths = collapse_affected_prefixes(
            changed_paths
                .into_iter()
                .filter(|path| self.path_belongs_to_index_roots(path))
                .collect(),
        );
        for changed_path in changed_paths {
            ensure_not_cancelled(cancellation)?;
            match self.observe_index_path(&changed_path)? {
                FilesystemObservation::Complete(entry)
                    if entry.kind() == SearchFileKind::Directory =>
                {
                    if self
                        .writer
                        .directory_snapshot(changed_path.clone())?
                        .is_some()
                    {
                        self.reconcile_directory(&changed_path, stats, cancellation, on_progress)
                            .await?;
                    } else {
                        self.delete_scope(&changed_path, false, stats, on_progress)?;
                        self.cold_scan_scope(&changed_path, stats, cancellation, on_progress)
                            .await?;
                    }
                }
                FilesystemObservation::Complete(entry) if entry.kind() == SearchFileKind::File => {
                    if self
                        .writer
                        .directory_snapshot(changed_path.clone())?
                        .is_some()
                    {
                        self.delete_scope(&changed_path, true, stats, on_progress)?;
                    }
                    self.index_observed_batch(
                        vec![entry],
                        &changed_path,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                }
                FilesystemObservation::Complete(_) => {
                    self.delete_scope(&changed_path, false, stats, on_progress)?;
                }
                FilesystemObservation::Inaccessible { scope } => {
                    self.mark_scope_inaccessible(&scope, false, stats, on_progress)?;
                }
                FilesystemObservation::Missing { scope }
                | FilesystemObservation::PolicyExcluded { scope } => {
                    let directory_changed =
                        self.writer.directory_snapshot(scope.clone())?.is_some();
                    self.delete_scope(&scope, directory_changed, stats, on_progress)?;
                }
            }
        }
        Ok(())
    }

    async fn check_known_file(
        &self,
        known_file: KnownFileEntry,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        stats.checked += 1;
        match self.observe_index_path(&known_file.path)? {
            FilesystemObservation::Complete(entry) if entry.kind() == SearchFileKind::File => {
                let observed_signature = observed_file_signature(entry.metadata());
                if known_file.state.allows_signature_skip(observed_signature) {
                    stats.skipped += 1;
                } else {
                    self.index_observed_batch(
                        vec![entry],
                        &known_file.path,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                }
            }
            FilesystemObservation::Complete(entry) if entry.kind() == SearchFileKind::Directory => {
                self.delete_scope(&known_file.path, false, stats, on_progress)?;
                self.cold_scan_scope(&known_file.path, stats, cancellation, on_progress)
                    .await?;
            }
            FilesystemObservation::Complete(_) => {
                self.delete_scope(&known_file.path, false, stats, on_progress)?;
            }
            FilesystemObservation::Inaccessible { scope } => {
                self.mark_scope_inaccessible(&scope, false, stats, on_progress)?;
            }
            FilesystemObservation::Missing { scope }
            | FilesystemObservation::PolicyExcluded { scope } => {
                self.delete_scope(&scope, false, stats, on_progress)?;
            }
        }
        report_checking_if_needed(stats, on_progress);
        Ok(())
    }

    async fn cold_scan_scope(
        &self,
        scope: &Path,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        let Some((root, boundary)) = self.observable_boundary_for_path(scope)? else {
            return Ok(());
        };
        match self.observe_index_path(scope)? {
            FilesystemObservation::Complete(entry) if entry.kind() == SearchFileKind::Directory => {
                self.crawl_directory_tree(&boundary, &root, scope, stats, cancellation, on_progress)
                    .await
            }
            FilesystemObservation::Complete(entry) if entry.kind() == SearchFileKind::File => {
                self.index_observed_batch(vec![entry], scope, stats, cancellation, on_progress)
                    .await
            }
            FilesystemObservation::Complete(_) => {
                self.delete_scope(scope, false, stats, on_progress)
            }
            FilesystemObservation::Inaccessible { scope } => {
                self.mark_scope_inaccessible(&scope, true, stats, on_progress)
            }
            FilesystemObservation::Missing { scope }
            | FilesystemObservation::PolicyExcluded { scope } => {
                self.delete_scope(&scope, true, stats, on_progress)
            }
        }
    }

    async fn crawl_directory_tree(
        &self,
        boundary: &LocalFilesystemBoundary,
        root: &Path,
        scope: &Path,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        let mut walker =
            match boundary.walk_directory(scope, TraversalDepth::Recursive, cancellation)? {
                FilesystemObservation::Complete(walker) => walker,
                FilesystemObservation::Inaccessible { scope } => {
                    self.mark_scope_inaccessible(&scope, true, stats, on_progress)?;
                    return Ok(());
                }
                FilesystemObservation::Missing { scope }
                | FilesystemObservation::PolicyExcluded { scope } => {
                    self.delete_scope(&scope, true, stats, on_progress)?;
                    return Ok(());
                }
            };
        let mut pending_entries = Vec::with_capacity(MAX_CLASSIFICATION_BATCH_ENTRIES);
        let mut pending_bytes = 0_usize;

        while let Some(event) = walker.next_event()? {
            ensure_not_cancelled(cancellation)?;
            match event {
                TraversalEvent::Entry(entry) if entry.kind() == SearchFileKind::File => {
                    push_pending_entry(
                        self,
                        entry,
                        scope,
                        &mut pending_entries,
                        &mut pending_bytes,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                }
                TraversalEvent::Entry(_) => {}
                TraversalEvent::Observation(FilesystemObservation::Complete(directory)) => {
                    self.index_observed_batch(
                        std::mem::take(&mut pending_entries),
                        scope,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                    pending_bytes = 0;
                    stats.directories_enumerated += 1;
                    self.reconcile_known_direct_children(&directory, stats, on_progress)?;
                    self.persist_directory_snapshot(
                        &directory,
                        root,
                        EntryObservationState::Observable,
                        stats,
                        on_progress,
                    )?;
                    report_crawling(stats, scope, on_progress);
                }
                TraversalEvent::Observation(FilesystemObservation::Inaccessible { scope }) => {
                    self.persist_directory_snapshot(
                        &scope,
                        root,
                        EntryObservationState::Inaccessible,
                        stats,
                        on_progress,
                    )?;
                    self.mark_scope_inaccessible(&scope, true, stats, on_progress)?;
                }
                TraversalEvent::Observation(FilesystemObservation::Missing { scope })
                | TraversalEvent::Observation(FilesystemObservation::PolicyExcluded { scope }) => {
                    self.delete_scope(&scope, true, stats, on_progress)?;
                }
            }
        }

        self.index_observed_batch(pending_entries, scope, stats, cancellation, on_progress)
            .await
    }

    async fn reconcile_directory(
        &self,
        directory: &Path,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        let Some((root, boundary)) = self.observable_boundary_for_path(directory)? else {
            return Ok(());
        };
        let mut walker = match boundary.walk_directory(
            directory,
            TraversalDepth::DirectChildren,
            cancellation,
        )? {
            FilesystemObservation::Complete(walker) => walker,
            FilesystemObservation::Inaccessible { scope } => {
                self.mark_scope_inaccessible(&scope, true, stats, on_progress)?;
                return Ok(());
            }
            FilesystemObservation::Missing { scope }
            | FilesystemObservation::PolicyExcluded { scope } => {
                self.delete_scope(&scope, true, stats, on_progress)?;
                return Ok(());
            }
        };
        let mut pending_entries = Vec::with_capacity(MAX_CLASSIFICATION_BATCH_ENTRIES);
        let mut pending_bytes = 0_usize;

        while let Some(event) = walker.next_event()? {
            ensure_not_cancelled(cancellation)?;
            match event {
                TraversalEvent::Entry(entry) if entry.kind() == SearchFileKind::File => {
                    if self
                        .writer
                        .directory_snapshot(entry.path().to_path_buf())?
                        .is_some()
                    {
                        self.delete_scope(entry.path(), true, stats, on_progress)?;
                    }
                    push_pending_entry(
                        self,
                        entry,
                        directory,
                        &mut pending_entries,
                        &mut pending_bytes,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                }
                TraversalEvent::Entry(entry) if entry.kind() == SearchFileKind::Directory => {
                    if self
                        .writer
                        .directory_snapshot(entry.path().to_path_buf())?
                        .is_none()
                    {
                        self.delete_scope(entry.path(), false, stats, on_progress)?;
                        self.crawl_directory_tree(
                            &boundary,
                            &root,
                            entry.path(),
                            stats,
                            cancellation,
                            on_progress,
                        )
                        .await?;
                    }
                }
                TraversalEvent::Entry(_) => {}
                TraversalEvent::Observation(FilesystemObservation::Complete(scope)) => {
                    self.index_observed_batch(
                        std::mem::take(&mut pending_entries),
                        directory,
                        stats,
                        cancellation,
                        on_progress,
                    )
                    .await?;
                    pending_bytes = 0;
                    stats.directories_enumerated += 1;
                    self.reconcile_known_direct_children(&scope, stats, on_progress)?;
                    self.persist_directory_snapshot(
                        &scope,
                        &root,
                        EntryObservationState::Observable,
                        stats,
                        on_progress,
                    )?;
                }
                TraversalEvent::Observation(FilesystemObservation::Inaccessible { scope }) => {
                    self.mark_scope_inaccessible(&scope, true, stats, on_progress)?;
                }
                TraversalEvent::Observation(FilesystemObservation::Missing { scope })
                | TraversalEvent::Observation(FilesystemObservation::PolicyExcluded { scope }) => {
                    self.delete_scope(&scope, true, stats, on_progress)?;
                }
            }
        }

        self.index_observed_batch(pending_entries, directory, stats, cancellation, on_progress)
            .await
    }

    fn reconcile_known_direct_children(
        &self,
        directory: &Path,
        stats: &mut RebuildStats,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        let mut after_path = None;
        loop {
            let page = self.writer.direct_children_page(
                directory.to_path_buf(),
                after_path.clone(),
                MAX_KNOWN_ENTRY_PAGE_ENTRIES,
            )?;
            if page.is_empty() {
                break;
            }
            after_path = page.last().map(|child| child.path.clone());
            for child in page {
                match self.observe_index_path(&child.path)? {
                    FilesystemObservation::Complete(entry) if entry.kind() == child.kind => {}
                    FilesystemObservation::Complete(_) => {
                        self.delete_scope(
                            &child.path,
                            child.kind == SearchFileKind::Directory,
                            stats,
                            on_progress,
                        )?;
                    }
                    FilesystemObservation::Inaccessible { scope } => {
                        self.mark_scope_inaccessible(
                            &scope,
                            child.kind == SearchFileKind::Directory,
                            stats,
                            on_progress,
                        )?;
                    }
                    FilesystemObservation::Missing { scope }
                    | FilesystemObservation::PolicyExcluded { scope } => {
                        self.delete_scope(
                            &scope,
                            child.kind == SearchFileKind::Directory,
                            stats,
                            on_progress,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) async fn index_observed_batch(
        &self,
        entries: Vec<FilesystemEntry>,
        current_scope: &Path,
        stats: &mut RebuildStats,
        cancellation: &CancellationToken,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        if entries.is_empty() {
            return Ok(());
        }
        ensure_not_cancelled(cancellation)?;

        let observed_files = entries
            .iter()
            .map(|entry| ObservedFile {
                path: entry.path().to_path_buf(),
                signature: observed_file_signature(entry.metadata()),
            })
            .collect::<Vec<_>>();
        let classifications = self.writer.classify_observed(observed_files)?;
        if classifications.len() != entries.len() {
            return Err(SearchError::WorkerFailed(
                "writer returned an incomplete classification batch".to_owned(),
            ));
        }

        let mut pending_mutations = Vec::new();
        let mut pending_mutation_bytes = 0_usize;
        for (entry, classification) in entries.into_iter().zip(classifications) {
            ensure_not_cancelled(cancellation)?;
            let observed_signature = observed_file_signature(entry.metadata());
            stats.scanned += 1;
            if classification
                .known_entry
                .as_ref()
                .is_some_and(|known_entry| known_entry.allows_signature_skip(observed_signature))
            {
                stats.skipped += 1;
                report_crawling_if_needed(stats, current_scope, on_progress);
                continue;
            }

            let changed_file_pipeline =
                self.plan_changed_file_pipeline(entry.path(), entry.metadata(), observed_signature);
            if extraction_plan_reads_content(&changed_file_pipeline.stage3_content_plan) {
                stats.content_reads += 1;
            }
            let pipeline_outcome = self
                .execute_changed_file_pipeline(entry.path(), changed_file_pipeline, cancellation)
                .await?;
            ensure_not_cancelled(cancellation)?;
            let mutation = match pipeline_outcome {
                ChangedFilePipelineOutcome::Observable(file) => ScanFileMutation::Observable(file),
                ChangedFilePipelineOutcome::Inaccessible { scope, file } => {
                    ScanFileMutation::Inaccessible { scope, file }
                }
            };
            let mutation_bytes = scan_file_mutation_bytes(&mutation);
            if !pending_mutations.is_empty()
                && pending_mutation_bytes.saturating_add(mutation_bytes)
                    > MAX_WRITER_FILE_PAYLOAD_BYTES
            {
                self.apply_file_mutations(
                    std::mem::take(&mut pending_mutations),
                    stats,
                    on_progress,
                )?;
                pending_mutation_bytes = 0;
            }
            pending_mutation_bytes = pending_mutation_bytes.saturating_add(mutation_bytes);
            pending_mutations.push(mutation);
            stats.changed += 1;
            stats.reindexed += 1;
            report_crawling_if_needed(stats, current_scope, on_progress);
        }

        self.apply_file_mutations(pending_mutations, stats, on_progress)
    }

    fn apply_file_mutations(
        &self,
        mutations: Vec<ScanFileMutation>,
        stats: &mut RebuildStats,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        if mutations.is_empty() {
            return Ok(());
        }
        on_progress(IndexMaintenanceProgress::Applying {
            pending_mutations: mutations.len() as u64,
        });
        stats.database_mutations = stats
            .database_mutations
            .saturating_add(mutations.len() as u64);
        self.writer.apply_file_batch(mutations)
    }

    fn persist_directory_snapshot(
        &self,
        directory: &Path,
        root: &Path,
        observation_state: EntryObservationState,
        stats: &mut RebuildStats,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        let FilesystemObservation::Complete(entry) = observe_path(directory)? else {
            return Ok(());
        };
        if entry.kind() != SearchFileKind::Directory {
            return Ok(());
        }
        let next_snapshot = DirectorySnapshot {
            path: directory.to_path_buf(),
            parent_path: directory.parent().unwrap_or(Path::new("")).to_path_buf(),
            root_path: root.to_path_buf(),
            signature: directory_signature(entry.metadata()),
            observation_state,
        };
        if self
            .writer
            .directory_snapshot(directory.to_path_buf())?
            .as_ref()
            == Some(&next_snapshot)
        {
            return Ok(());
        }
        on_progress(IndexMaintenanceProgress::Applying {
            pending_mutations: 1,
        });
        self.writer.upsert_directory_snapshot(next_snapshot)?;
        stats.database_mutations += 1;
        stats.directory_snapshots_changed += 1;
        Ok(())
    }

    fn mark_scope_inaccessible(
        &self,
        scope: &Path,
        directory_changed: bool,
        stats: &mut RebuildStats,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        if !self.writer.mark_scope_inaccessible(scope.to_path_buf())? {
            return Ok(());
        }
        on_progress(IndexMaintenanceProgress::Applying {
            pending_mutations: 1,
        });
        stats.changed += 1;
        stats.database_mutations += 1;
        if directory_changed {
            stats.directory_snapshots_changed += 1;
        }
        Ok(())
    }

    fn delete_scope(
        &self,
        scope: &Path,
        directory_changed: bool,
        stats: &mut RebuildStats,
        on_progress: &mut impl FnMut(IndexMaintenanceProgress),
    ) -> SearchResult<()> {
        on_progress(IndexMaintenanceProgress::Applying {
            pending_mutations: 1,
        });
        self.writer.delete_scope(scope.to_path_buf())?;
        stats.changed += 1;
        stats.database_mutations += 1;
        if directory_changed {
            stats.directory_snapshots_changed += 1;
        }
        Ok(())
    }

    fn observe_index_path(
        &self,
        path: &Path,
    ) -> SearchResult<FilesystemObservation<FilesystemEntry>> {
        let Some((root, root_device)) = self
            .root_devices
            .iter()
            .filter(|(root, _)| path_is_same_or_descendant(path, root))
            .max_by_key(|(root, _)| root.components().count())
        else {
            return Ok(FilesystemObservation::PolicyExcluded {
                scope: path.to_path_buf(),
            });
        };
        let Some(root_device) = root_device else {
            return observe_path(path);
        };
        match observe_path(path)? {
            FilesystemObservation::Complete(entry) => {
                let policy_excluded = entry.metadata().dev() != *root_device
                    || (path != root.as_path()
                        && match entry.kind() {
                            SearchFileKind::Directory => self.rules.should_skip_directory(path),
                            SearchFileKind::File => self.rules.should_skip_path(path),
                            _ => true,
                        });
                if policy_excluded {
                    Ok(FilesystemObservation::PolicyExcluded {
                        scope: path.to_path_buf(),
                    })
                } else {
                    Ok(FilesystemObservation::Complete(entry))
                }
            }
            observation => Ok(observation),
        }
    }

    fn observable_boundary_for_path(
        &self,
        path: &Path,
    ) -> SearchResult<Option<(PathBuf, LocalFilesystemBoundary)>> {
        let Some(root) = self
            .config
            .roots
            .iter()
            .filter(|root| path_is_same_or_descendant(path, root))
            .max_by_key(|root| root.components().count())
            .cloned()
        else {
            return Ok(None);
        };
        match LocalFilesystemBoundary::observe(&root, &self.rules)? {
            FilesystemObservation::Complete(boundary) => Ok(Some((root, boundary))),
            FilesystemObservation::Inaccessible { scope } => {
                self.writer.mark_scope_inaccessible(scope)?;
                Ok(None)
            }
            FilesystemObservation::Missing { scope }
            | FilesystemObservation::PolicyExcluded { scope } => {
                self.writer.delete_scope(scope)?;
                Ok(None)
            }
        }
    }
}
