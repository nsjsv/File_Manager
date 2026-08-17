use super::*;

use crate::model::Message;
use crate::operation_queue::{
    execute_file_operation_persistence, FileOperationEnqueueOutcome, FileOperationQueue,
    QueuedFileOperation, QueuedTransfer,
};
use file_operation_store::TaskQueueStore;
use iced::futures::channel::mpsc;
use iced::futures::StreamExt;
use rusqlite::Connection;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CORPUS_SIZE: usize = 256 * 1024 * 1024;
const WRITER_LOCK: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq)]
enum HarnessOperation {
    Copy,
    Move,
}

impl HarnessOperation {
    fn queued_mode(self) -> QueuedTransferMode {
        match self {
            Self::Copy => QueuedTransferMode::Copy,
            Self::Move => QueuedTransferMode::Move,
        }
    }

    fn queued_operation(
        self,
        transfer: QueuedTransfer,
        verification: FileOperationVerification,
    ) -> QueuedFileOperation {
        match self {
            Self::Copy => QueuedFileOperation::Copy {
                transfers: vec![transfer],
                verification,
            },
            Self::Move => QueuedFileOperation::Move {
                transfers: vec![transfer],
                verification,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct HarnessVerification {
    value: FileOperationVerification,
    name: &'static str,
}

const VERIFICATIONS: [HarnessVerification; 2] = [
    HarnessVerification {
        value: FileOperationVerification::BasicMetadata,
        name: "basic",
    },
    HarnessVerification {
        value: FileOperationVerification::Strong,
        name: "strong",
    },
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "release-only startup latency harness; run explicitly with --ignored --nocapture"]
async fn file_operation_start_latency_release_harness() {
    let corpus_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/file-operation-start-latency-corpus")
        .join(std::process::id().to_string());
    fs::create_dir_all(&corpus_root).unwrap();
    assert_disk_backed(&corpus_root);

    for operation in [HarnessOperation::Copy, HarnessOperation::Move] {
        for verification in VERIFICATIONS {
            run_scenario(&corpus_root, operation, verification).await;
        }
    }

    fs::remove_dir_all(corpus_root).unwrap();
}

async fn run_scenario(
    corpus_root: &Path,
    operation: HarnessOperation,
    verification: HarnessVerification,
) {
    let scenario_name = match operation {
        HarnessOperation::Copy => "copy",
        HarnessOperation::Move => "move",
    };
    let scenario_root = corpus_root.join(format!("{scenario_name}-{}", verification.name));
    fs::create_dir_all(&scenario_root).unwrap();
    let source = scenario_root.join("source.bin");
    let target = scenario_root.join("target.bin");
    write_corpus(&source);

    let store = TaskQueueStore::new(scenario_root.join("state.sqlite")).unwrap();
    let transfer = QueuedTransfer::new(source.clone(), target.clone());
    let mut queue = FileOperationQueue::new();
    queue.set_store_with_deferred_persistence(store);

    let database_path = scenario_root.join("state.sqlite");
    let (writer_ready_tx, writer_ready_rx) = std::sync::mpsc::channel();
    let (writer_finished_tx, writer_finished_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let mut connection = Connection::open(database_path).unwrap();
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        writer_ready_tx.send(()).unwrap();
        std::thread::sleep(WRITER_LOCK);
        drop(transaction);
        writer_finished_tx.send(Instant::now()).unwrap();
    });
    writer_ready_rx.recv().unwrap();

    let measurement_start = Instant::now();
    let acceptance_start = Instant::now();
    let outcome = queue.enqueue(operation.queued_operation(transfer.clone(), verification.value));
    let acceptance_finished = Instant::now();
    let _local_task_id = match outcome {
        FileOperationEnqueueOutcome::Queued { task_id }
        | FileOperationEnqueueOutcome::QueuedWithStorageWarning { task_id, .. } => task_id,
        FileOperationEnqueueOutcome::Rejected { error } => {
            panic!("harness enqueue failed: {error}")
        }
    };

    let request = queue
        .take_next_persistence_request()
        .expect("enqueue must produce a persistence request");
    let persistence_started = Instant::now();
    let persistence_outcome =
        tokio::task::spawn_blocking(move || execute_file_operation_persistence(request))
            .await
            .unwrap();
    let persistence_finished = Instant::now();
    let writer_released = writer_finished_rx.recv().unwrap();
    writer.join().unwrap();

    assert_eq!(persistence_outcome.request_id, 1);
    let persistence_acceptance = queue.accept_persistence_outcome(persistence_outcome);
    assert!(persistence_acceptance.error.is_none());
    assert_eq!(queue.tasks()[0].status_label(), "Preparing");

    // Drain the Running status write before recording runner start.
    while let Some(request) = queue.take_next_persistence_request() {
        let outcome =
            tokio::task::spawn_blocking(move || execute_file_operation_persistence(request))
                .await
                .unwrap();
        let acceptance = queue.accept_persistence_outcome(outcome);
        assert!(acceptance.error.is_none());
    }

    let running = queue
        .active_subscription()
        .expect("runner must be observable");
    let stored_task_id = running.stored_id.expect("recoverable task must be durable");
    let task_id = running.id;
    let controls = running.controls;
    let store = running.store;
    let (mut output, mut messages) = mpsc::channel(32);
    let first_side_effect = Arc::new(Mutex::new(None));
    let observed_side_effect = Arc::clone(&first_side_effect);
    let observed_source = source.clone();
    let observed_target = target.clone();
    let observed_parent = scenario_root.to_path_buf();
    let mut observer = tokio::spawn(async move {
        loop {
            let side_effect_seen = match operation {
                HarnessOperation::Move => !observed_source.exists() || observed_target.exists(),
                HarnessOperation::Copy => {
                    let mut entries = tokio::fs::read_dir(&observed_parent).await.unwrap();
                    let mut payload_written = false;
                    while let Some(entry) = entries.next_entry().await.unwrap() {
                        if !entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".file-manager-transfer-")
                        {
                            continue;
                        }
                        let payload = entry.path().join("payload");
                        if tokio::fs::metadata(payload)
                            .await
                            .is_ok_and(|metadata| metadata.len() > 0)
                        {
                            payload_written = true;
                            break;
                        }
                    }
                    payload_written
                }
            };
            if side_effect_seen {
                let mut timestamp = observed_side_effect.lock().unwrap();
                if timestamp.is_none() {
                    *timestamp = Some(Instant::now());
                }
                return;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    let runner_started = Instant::now();
    let run_task = tokio::spawn(async move {
        run_queued_transfers(
            vec![transfer],
            controls,
            stored_task_id,
            task_id,
            &mut output,
            store,
            operation.queued_mode(),
            verification.value,
        )
        .await
    });
    let mut first_progress = None;
    while let Some(message) = messages.next().await {
        if matches!(message, Message::FileOperationProgressed(_, _)) && first_progress.is_none() {
            first_progress = Some(Instant::now());
        }
    }
    let completion = run_task.await.unwrap();
    let runner_finished = Instant::now();
    match tokio::time::timeout(Duration::from_secs(1), &mut observer).await {
        Ok(joined) => {
            joined.unwrap();
        }
        Err(_) => {
            observer.abort();
            let _ = observer.await;
        }
    }
    assert!(matches!(completion, FileOperationCompletion::Succeeded(_)));

    let first_side_effect = first_side_effect
        .lock()
        .unwrap()
        .as_ref()
        .copied()
        .expect("harness did not observe first filesystem side effect");
    if operation == HarnessOperation::Copy {
        assert!(
            first_progress.is_some(),
            "harness did not observe first copy progress"
        );
    }
    println!(
        "start_latency scenario={scenario_name} verification={} accepted_ms={:.3} persistence_started_ms={:.3} persistence_finished_ms={:.3} runner_started_ms={:.3} first_progress_ms={} first_side_effect_ms={:.3} runner_finished_ms={:.3} ui_handler_ms={:.3} sqlite_worker_ms={:.3} sqlite_lock_wait_ms={:.3} core_preparation_ms={:.3}",
        verification.name,
        elapsed_ms(measurement_start, acceptance_finished),
        elapsed_ms(measurement_start, persistence_started),
        elapsed_ms(measurement_start, persistence_finished),
        elapsed_ms(measurement_start, runner_started),
        optional_elapsed_ms(runner_started, first_progress),
        elapsed_ms(measurement_start, first_side_effect),
        elapsed_ms(measurement_start, runner_finished),
        elapsed_ms(acceptance_start, acceptance_finished),
        elapsed_ms(persistence_started, persistence_finished),
        elapsed_ms(persistence_started, writer_released),
        elapsed_ms(runner_started, first_side_effect),
    );

    assert!(
        acceptance_finished.duration_since(acceptance_start) <= Duration::from_millis(50),
        "UI acceptance exceeded 50 ms"
    );
    if verification.value == FileOperationVerification::BasicMetadata {
        assert!(
            first_side_effect.duration_since(runner_started) <= Duration::from_millis(100),
            "Basic {scenario_name} first side effect exceeded 100 ms"
        );
    }
}

fn write_corpus(path: &Path) {
    let mut file = File::create(path).unwrap();
    let chunk = vec![0x5a_u8; 1024 * 1024];
    for _ in 0..(CORPUS_SIZE / chunk.len()) {
        file.write_all(&chunk).unwrap();
    }
    file.sync_all().unwrap();
}

fn optional_elapsed_ms(start: Instant, end: Option<Instant>) -> String {
    end.map(|end| format!("{:.3}", elapsed_ms(start, end)))
        .unwrap_or_else(|| "na".to_owned())
}

fn elapsed_ms(start: Instant, end: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1_000.0
}

#[cfg(target_os = "linux")]
fn assert_disk_backed(path: &Path) {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;

    const TMPFS_MAGIC: u64 = 0x0102_1994;
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    let status = unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) };
    assert_eq!(status, 0, "cannot inspect harness filesystem");
    let stat = unsafe { stat.assume_init() };
    assert_ne!(
        stat.f_type as u64, TMPFS_MAGIC,
        "harness corpus must not use tmpfs"
    );
}

#[cfg(not(target_os = "linux"))]
fn assert_disk_backed(_path: &Path) {}
