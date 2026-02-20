//! rsync transfer execution and concurrency control.

use std::collections::{HashMap, HashSet};

use tokio::process::Command;
use tokio::sync::{Mutex, Semaphore, SemaphorePermit, TryAcquireError};

use crate::config::DaemonConfig;
use crate::task::TransferTaskInfo;

/// Executes rsync transfer tasks from seedbox to relay with transfer de-duplication.
///
/// # Objectives
///
/// - Prevent duplicate execution for the same torrent hash.
/// - Keep transfer orchestration simple in daemon code.
/// - Preserve compatibility with future multi-session scheduling.
///
/// Currently we only allow one session at a time to avoid overloading the
/// bandwidth of home network.
///
/// # Deliberate behavior
///
/// We allow duplicate planning across sessions, i.e. multiple sessions can register the same hash
/// to queue.
/// This is useful, e.g. for cron session vs. user-requested high-priority session.
/// Only duplicate execution is prevented.
///
/// # Concurrency model
///
/// - Session acquisition is currently guarded by a semaphore permit of capacity 1.
/// - Creating a session records planned hashes in `queued_counts`.
/// - Per-hash `try_guard(hash)` is non-blocking and grants exclusive execution.
/// - `TransferGuard` marks hash as in-progress; caller must invoke `release()`.
/// - Releasing `RsyncTransferSession` decrements queued counts for all hashes
///   planned by that session.
///
/// # Integration contract
///
/// The caller must perform status check, rsync execution, and storage state
/// transition (`mark ready`) within a single `TransferGuard` lifetime.
/// Correctness relies on storage transitions being concurrency-safe and
/// atomic.
pub struct RsyncTransmitter {
    executor: RsyncExecutor,
    gate: Semaphore,
    tracker: TransferTracker,
}

struct RsyncExecutor {
    ssh_host: String,
    ssh_key_path: String,
    speed_limit: Option<u32>,
    dry_run: bool,
}

struct TransferTracker {
    state: Mutex<TransferTrackerState>,
}

#[derive(Default)]
struct TransferTrackerState {
    queued_counts: HashMap<String, usize>,
    in_progress: HashSet<String>,
}

pub struct RsyncTransferSession<'a> {
    _permit: SemaphorePermit<'a>,
    executor: &'a RsyncExecutor,
    tracker: &'a TransferTracker,
    task_hashes: HashSet<String>,
    tasks: Vec<TransferTaskInfo>,
    released: bool,
}

/// Per-hash execution guard.
///
/// While held, the hash is considered in-progress and other sessions trying to
/// acquire guard for the same hash will fail with `Ok(None)`.
///
/// Guard state is finalized by calling `release(self)`. Dropping without
/// `release()` triggers `debug_assert!` in debug builds.
pub struct TransferGuard<'a> {
    hash: String,
    tracker: &'a TransferTracker,
    executor: &'a RsyncExecutor,
    released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    None,
    Queued,
    Ongoing,
}

impl RsyncTransmitter {
    /// Build a transmitter from daemon configuration.
    pub fn from_config(config: &DaemonConfig) -> Self {
        tracing::debug!(host = %config.seedbox_ssh_host, dry_run = config.no_transfer, "Creating rsync transmitter");

        Self {
            executor: RsyncExecutor::from_config(config),
            gate: Semaphore::new(1),
            tracker: TransferTracker::new(),
        }
    }

    /// Acquire a transfer session, waiting until the session gate is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the session semaphore is closed.
    #[allow(dead_code)]
    pub async fn session(
        &self,
        tasks: Vec<TransferTaskInfo>,
    ) -> Result<RsyncTransferSession<'_>, RsyncTransmitterError> {
        let permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| RsyncTransmitterError::SemaphoreClosed)?;

        Ok(self.make_session(tasks, permit).await)
    }

    /// Try to acquire a transfer session without blocking.
    ///
    /// # Errors
    ///
    /// Returns [`RsyncTransmitterError::OverlappingTransfer`] when another
    /// session currently holds the gate.
    pub async fn try_session(
        &self,
        tasks: Vec<TransferTaskInfo>,
    ) -> Result<RsyncTransferSession<'_>, RsyncTransmitterError> {
        let permit = self.gate.try_acquire().map_err(|e| match e {
            TryAcquireError::Closed => RsyncTransmitterError::SemaphoreClosed,
            TryAcquireError::NoPermits => RsyncTransmitterError::OverlappingTransfer,
        })?;

        Ok(self.make_session(tasks, permit).await)
    }

    /// Create a transfer session and register its planned hashes as queued.
    pub async fn make_session<'a>(
        &'a self,
        tasks: Vec<TransferTaskInfo>,
        permit: SemaphorePermit<'a>,
    ) -> RsyncTransferSession<'a> {
        self.tracker
            .enqueue_many(tasks.iter().map(|task| task.hash.as_str()))
            .await;
        let task_hashes = tasks.iter().map(|task| task.hash.clone()).collect();

        RsyncTransferSession {
            _permit: permit,
            executor: &self.executor,
            tracker: &self.tracker,
            task_hashes,
            tasks,
            released: false,
        }
    }

    /// Return current transfer state for a hash.
    pub async fn transfer_state(&self, hash: &str) -> TransferState {
        self.tracker.state_of(hash).await
    }
}

impl RsyncTransferSession<'_> {
    /// Borrow transfer tasks planned in this session.
    pub fn tasks(&self) -> &[TransferTaskInfo] {
        &self.tasks
    }

    /// Try to acquire per-hash transfer guard without blocking.
    ///
    /// Returns:
    /// - `Ok(Some(_))` if this session now owns execution for `hash`
    /// - `Ok(None)` if another session is already executing `hash`
    /// - `Err(UnclaimedHash)` if `hash` is not part of this session
    pub async fn try_guard(
        &self,
        hash: &str,
    ) -> Result<Option<TransferGuard<'_>>, RsyncTransmitterError> {
        if !self.task_hashes.contains(hash) {
            return Err(RsyncTransmitterError::UnclaimedHash {
                hash: hash.to_string(),
            });
        }

        if !self.tracker.try_start(hash).await? {
            return Ok(None);
        }

        Ok(Some(TransferGuard {
            hash: hash.to_string(),
            tracker: self.tracker,
            executor: self.executor,
            released: false,
        }))
    }

    /// Release queued ownership for hashes planned by this session.
    ///
    /// This must be called before the session is dropped.
    pub async fn release(mut self) {
        self.released = true;

        let pending_hashes = self
            .tasks
            .iter()
            .map(|task| task.hash.clone())
            .collect::<Vec<_>>();
        self.tracker
            .release_queued_many(pending_hashes.iter().map(String::as_str))
            .await;
    }
}

impl Drop for RsyncTransferSession<'_> {
    fn drop(&mut self) {
        debug_assert!(
            self.released,
            "RsyncTransferSession dropped without release()"
        );
    }
}

impl TransferGuard<'_> {
    /// Execute rsync while this guard is held.
    ///
    /// Callers should perform status check and storage state transition under
    /// the same guard lifetime to avoid re-transfer races.
    pub async fn transfer(&self, task: &TransferTaskInfo) -> Result<(), RsyncTransmitterError> {
        self.executor.transfer(task).await
    }

    /// Release in-progress ownership for this hash.
    ///
    /// This must be called before the guard is dropped.
    pub async fn release(mut self) {
        self.released = true;

        self.tracker.finish_transfer(&self.hash).await;
    }
}

impl Drop for TransferGuard<'_> {
    fn drop(&mut self) {
        debug_assert!(self.released, "TransferGuard dropped without release()");
    }
}

impl TransferTracker {
    /// Create empty transfer tracker state.
    fn new() -> Self {
        Self {
            state: Mutex::new(TransferTrackerState::default()),
        }
    }

    /// Increase queued counters for planned hashes.
    async fn enqueue_many<'a>(&self, hashes: impl Iterator<Item = &'a str>) {
        let mut state = self.lock_state().await;
        for hash in hashes {
            *state.queued_counts.entry(hash.to_string()).or_insert(0) += 1;
        }
    }

    /// Decrease queued counters for hashes released by a session.
    async fn release_queued_many<'a>(&self, hashes: impl Iterator<Item = &'a str>) {
        let mut state = self.lock_state().await;
        for hash in hashes {
            state.decrement_queued_count(hash);
        }
    }

    /// Try to mark a queued hash as in-progress.
    ///
    /// Returns `Ok(false)` if already in progress in another session.
    async fn try_start(&self, hash: &str) -> Result<bool, RsyncTransmitterError> {
        let mut state = self.lock_state().await;
        if !state.queued_counts.contains_key(hash) {
            return Err(RsyncTransmitterError::UnclaimedHash {
                hash: hash.to_string(),
            });
        }

        if state.in_progress.contains(hash) {
            return Ok(false);
        }

        state.in_progress.insert(hash.to_string());
        drop(state);
        Ok(true)
    }

    /// Remove a hash from in-progress set, marking transfer completion.
    async fn finish_transfer(&self, hash: &str) {
        self.lock_state().await.in_progress.remove(hash);
    }

    /// Returns current transfer state for a hash.
    async fn state_of(&self, hash: &str) -> TransferState {
        let state = self.lock_state().await;
        if state.in_progress.contains(hash) {
            TransferState::Ongoing
        } else if state.queued_counts.contains_key(hash) {
            TransferState::Queued
        } else {
            TransferState::None
        }
    }

    /// Lock tracker state.
    async fn lock_state(&self) -> tokio::sync::MutexGuard<'_, TransferTrackerState> {
        self.state.lock().await
    }
}

impl TransferTrackerState {
    /// Decrement queued counter for a hash and remove zero entries.
    fn decrement_queued_count(&mut self, hash: &str) {
        let remove = self.queued_counts.get_mut(hash).is_some_and(|count| {
            *count -= 1;
            *count == 0
        });
        if remove {
            self.queued_counts.remove(hash);
        }
    }
}

impl RsyncExecutor {
    /// Build rsync executor from daemon configuration.
    pub fn from_config(config: &DaemonConfig) -> Self {
        tracing::debug!(host = %config.seedbox_ssh_host, dry_run = config.no_transfer, "Creating rsync transmitter");
        Self {
            ssh_host: config.seedbox_ssh_host.clone(),
            ssh_key_path: config.seedbox_ssh_key.to_string_lossy().to_string(),
            speed_limit: config.rsync_speed_limit,
            dry_run: config.no_transfer,
        }
    }

    /// Execute a single rsync transfer task.
    ///
    /// # Errors
    ///
    /// Returns an error when rsync execution fails or exits unsuccessfully.
    pub async fn transfer(&self, task: &TransferTaskInfo) -> Result<(), RsyncTransmitterError> {
        let source = task.source.as_str();
        let dest = task.dest.as_str();

        tracing::info!(source = %source, dest = %dest, "Transferring files");

        let mut rsync_cmd = Command::new("rsync");
        rsync_cmd.args([
            "--delete",
            "--partial",
            "--recursive",
            "-s", // `--protect-args` / `--secluded-args`, use short version for compatibility.
        ]);

        if let Some(limit) = self.speed_limit {
            rsync_cmd.arg("--bwlimit").arg(limit.to_string());
        }

        let ssh_cmd = shell_words::join([
            "ssh",
            "-i",
            &self.ssh_key_path,
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
        ]);
        rsync_cmd.arg("--rsh").arg(ssh_cmd);

        // Because we use `-s`, no need to escape manually.
        let source_arg = &format!("{}:{}", self.ssh_host, source);
        rsync_cmd.arg(source_arg);
        rsync_cmd.arg(dest);

        tracing::debug!(command = ?rsync_cmd, "Executing rsync command");
        if self.dry_run {
            tracing::info!(source = %source, dest = %dest, "Skipping transfer in dry_run mode");
            return Ok(());
        }

        rsync_cmd
            .output()
            .await
            .map_err(|e| RsyncTransmitterError::RsyncFailed {
                dest: dest.to_string(),
                reason: format!("failed to execute rsync command: {e}"),
            })
            .and_then(|output| {
                if output.status.success() {
                    Ok(output)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Err(RsyncTransmitterError::RsyncFailed {
                        dest: dest.to_string(),
                        reason: format!("rsync exited with code {}: {}", output.status, stderr),
                    })
                }
            })?;

        tracing::info!(dest = %dest, "Artifact available");

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RsyncTransmitterError {
    #[error("semaphore closed")]
    SemaphoreClosed,
    #[error("Rsync command failed for {dest}: {reason}")]
    RsyncFailed { dest: String, reason: String },
    #[error("Another transfer job is already in progress")]
    OverlappingTransfer,
    #[error("Hash is not part of current transfer session: {hash}")]
    UnclaimedHash { hash: String },
}
