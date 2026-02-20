use std::sync::Arc;

use tokio::task::JoinHandle;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::{
    api::ApiServer,
    bot::{BotCommand, ReportTorrentInfo, TelegramBot},
    config::DaemonConfig,
    qbit::QBitSeedbox,
    rsync::{RsyncTransferSession, RsyncTransmitter, RsyncTransmitterError, TransferGuard},
    storage::{StorageManager, StorageManagerError, TaskStatus},
    task::TransferTaskInfo,
};

pub struct AniplerDaemon {
    api: ApiServer,
    bot: TelegramBot,
    config: DaemonConfig,
    seedbox: QBitSeedbox,
    store: Arc<StorageManager>,
    transmitter: RsyncTransmitter,
}

impl AniplerDaemon {
    /// Create a new daemon instance from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization of any component fails.
    pub async fn from_config(config: DaemonConfig) -> anyhow::Result<Arc<Self>> {
        tracing::info!("Initializing Anipler daemon");

        tracing::debug!("Initializing seedbox connection");
        let seedbox = QBitSeedbox::from_config(&config);

        tracing::debug!("Initializing storage manager");
        let store = StorageManager::from_config(&config).await?;
        let store_arc = Arc::new(store);

        tracing::debug!("Initializing rsync transmitter");
        let transmitter = RsyncTransmitter::from_config(&config);

        tracing::debug!("Initializing Telegram bot");
        let bot = TelegramBot::from_config(&config);

        tracing::debug!("Initializing API server");
        let api = ApiServer::from_config(&config, store_arc.clone());

        let daemon = Self {
            api,
            bot,
            config,
            seedbox,
            store: store_arc,
            transmitter,
        };

        tracing::info!("Anipler daemon initialized successfully");

        Ok(Arc::new(daemon))
    }

    /// Run the main event loop of the daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if anything fails during startup, or an unrecoverable error occurs during runtime.
    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let cancel = CancellationToken::new();

        let jobs_handle = self.clone().run_jobs().await?;

        self.run_pull_job().await;

        self.bot.run().await?;

        tracing::info!("Daemon main loop started");

        let mut api_handle = self.api.run(cancel.clone())?;
        let mut api_exited = false;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!(signal = "Ctrl-C", "Received shutdown signal");

                    cancel.cancel();

                    break;
                }
                cmd = self.bot.recv_command() => {
                    match cmd {
                        Ok(cmd) => self.clone().handle_command(cmd),
                        Err(crate::bot::TelegramBotError::ChannelClosed) => {
                            tracing::error!(reason = "channel_closed", "Telegram bot command channel closed unexpectedly, shutting down");
                            cancel.cancel();
                            break;
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, "Failed to receive command");
                        }
                    }
                }
                res = &mut api_handle => {
                    api_exited = true;
                    match res {
                        Ok(Ok(())) => {
                            // SAFETY: the first `Ok()` indicates future completes successfully,
                            // the second `Ok()` indicates axum returns no error,
                            // which further indicates graceful shutdown triggered by cancel token.
                            // However, on shutdown signal, we trigger cancel token and break this
                            // loop, `select!` should not be called again.
                            unreachable!("Main loop continued after shutdown signal");
                        },
                        Ok(Err(e)) => {
                            tracing::error!(error = ?e, "Join failed, API server might have crashed");
                        },
                        Err(e) => {
                            tracing::error!(error = ?e, "Axum server returned undocumented error");
                        }
                    }
                    break;
                }
            };
        }

        cancel.cancel();

        if !api_exited {
            let res = api_handle.await;
            if let Err(e) = res {
                tracing::error!(error = ?e, "API server error during shutdown");
            }
        }

        jobs_handle.await??;
        self.bot.shutdown().await?;

        tracing::info!(reason = "graceful_shutdown", "Daemon terminated");

        Ok(())
    }

    /// Run scheduled jobs for pulling torrents status and transferring ready torrents.
    ///
    /// # Errors
    ///
    /// Returns an error if job creation fails.
    pub async fn run_jobs(
        self: Arc<Self>,
    ) -> anyhow::Result<JoinHandle<Result<(), JobSchedulerError>>> {
        let sched = JobScheduler::new().await?;
        sched.shutdown_on_ctrl_c();

        let pull_job = {
            let daemon = self.clone();
            tokio_cron_scheduler::Job::new_async_tz(
                &self.config.pull_cron,
                chrono::Local,
                move |_, _| {
                    Box::pin({
                        let daemon = daemon.clone();
                        async move {
                            daemon.run_pull_job().await;
                        }
                    })
                },
            )?
        };
        sched.add(pull_job).await?;

        let transfer_job = {
            let daemon = self.clone();
            tokio_cron_scheduler::Job::new_async_tz(
                &self.config.transfer_cron,
                chrono::Local,
                move |_, _| {
                    Box::pin({
                        let daemon = daemon.clone();
                        async move {
                            daemon.run_transfer_job().await;
                        }
                    })
                },
            )?
        };
        sched.add(transfer_job).await?;

        let handle = tokio::spawn(async move { sched.start().await });

        Ok(handle)
    }

    /// Wrapper around pulling jobs with errors caught and logged.
    #[instrument(skip(self))]
    pub async fn run_pull_job(&self) {
        tracing::info!(
            operation = "pull_torrents",
            "Starting torrent information pull from seedbox"
        );

        self.update_status()
            .await
            .unwrap_or_else(|e| tracing::error!(error = ?e, "Failed to pull torrents information"));
    }

    /// Wrapper around transfer jobs with errors caught and logged.
    #[instrument(skip(self))]
    // False positive on `session`, which will be consumed in `release`.
    #[allow(clippy::significant_drop_tightening)]
    pub async fn run_transfer_job(&self) {
        tracing::info!("Starting transfer of ready torrents");

        let ready_torrents = match self.store.list_ready_torrents().await {
            Ok(ready_torrents) => ready_torrents,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to list ready torrents");
                return;
            }
        };
        let transfer_tasks = ready_torrents
            .iter()
            .map(|torrent| TransferTaskInfo {
                hash: torrent.hash.clone(),
                source: torrent.content_path.clone(),
                dest: self
                    .store
                    .artifact_storage_path(&torrent.hash)
                    .to_string_lossy()
                    .to_string(),
                name: torrent.name.clone(),
            })
            .collect::<Vec<_>>();
        let total_count = transfer_tasks.len();
        tracing::info!(count = total_count, "Found ready torrents for transfer");

        let session = match self.transmitter.try_session(transfer_tasks).await {
            Ok(session) => session,
            Err(RsyncTransmitterError::OverlappingTransfer) => {
                tracing::warn!(
                    state = "overlapping_job",
                    "Another transfer job is already in progress, skipping"
                );
                return;
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to acquire transfer session");
                return;
            }
        };

        let mut transferred = 0;
        for task in session.tasks() {
            match self.transfer_task(&session, task).await {
                Ok(true) => {
                    transferred += 1;
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::error!(error = ?e, "Transfer task failed");
                    self.bot.notify_transfer_failure(task, &e.to_string()).await;
                }
            }
        }
        session.release().await;

        tracing::info!(count = transferred, "Transfer job completed successfully");
    }

    /// Execute one transfer task inside per-hash transfer guard.
    ///
    /// Returns `Ok(true)` when transfer finished and was marked ready,
    /// `Ok(false)` when transfer is skipped due to state/concurrency,
    /// and `Err(_)` for hard failures.
    async fn transfer_task(
        &self,
        transmitter: &RsyncTransferSession<'_>,
        task: &TransferTaskInfo,
    ) -> Result<bool, AniplerDaemonError> {
        let hash = &task.hash;
        tracing::info!(torrent = %task.name, hash = %hash, "Starting torrent transfer");

        let Some(transfer_guard) = transmitter.try_guard(hash).await? else {
            tracing::trace!(torrent = %task.name, hash = %hash, "Skipping transfer: hash is in progress by another session");
            return Ok(false);
        };

        if self.store.task_status_by_hash(hash).await? != Some(TaskStatus::TorrentReady) {
            tracing::info!(torrent = %task.name, hash = %hash, "Skipping transfer: perhaps artifact is already ready");
            transfer_guard.release().await;
            return Ok(false);
        }

        if !self.config.no_transfer {
            self.bot.notify_transfer_start(task).await;
        }

        let transfer_result = self.transfer_and_mark_ready(&transfer_guard, task).await;
        transfer_guard.release().await;
        transfer_result?;

        if !self.config.no_transfer {
            self.bot.notify_transfer_completion(task).await;
        }

        Ok(true)
    }

    /// Perform storage preparation, rsync transfer, and ready-state update.
    ///
    /// This function is expected to run under an acquired [`TransferGuard`].
    async fn transfer_and_mark_ready(
        &self,
        transfer_guard: &TransferGuard<'_>,
        task: &TransferTaskInfo,
    ) -> Result<(), AniplerDaemonError> {
        let hash = &task.hash;

        self.store.prepare_artifact_storage(hash).await?;

        // Transmitter handles `no_transfer` flag internally.
        transfer_guard.transfer(task).await?;

        if !self.config.no_transfer {
            self.store.mark_artifact_ready(hash).await?;
        }

        Ok(())
    }

    /// Build and send `/report` message to Telegram chat.
    #[instrument(skip(self))]
    pub async fn run_report_job(&self) {
        let _result: anyhow::Result<()> = async {
            let torrents = self.store.list_ready_torrents().await?;
            let artifacts = self.store.list_ready_artifacts().await?;
            let mut report_torrents = Vec::with_capacity(torrents.len());
            for torrent in &torrents {
                report_torrents.push(ReportTorrentInfo {
                    hash: torrent.hash.clone(),
                    name: torrent.name.clone(),
                    transfer_state: self.transmitter.transfer_state(&torrent.hash).await,
                });
            }
            self.bot
                .report_available(&report_torrents, &artifacts)
                .await?;
            Ok(())
        }
        .await
        .inspect_err(|e| {
            tracing::error!(error = ?e, "Failed to run report job");
        });
    }

    /// Fetch the latest torrent status from the seedbox and update the local storage.
    ///
    /// # Errors
    ///
    /// Returns an error if querying the seedbox or updating the storage fails.
    pub async fn update_status(&self) -> anyhow::Result<()> {
        tracing::debug!("Querying seedbox for torrent updates");

        let earliest_import_date = self.store.earliest_import_date().await?;
        tracing::trace!(earliest_date = %earliest_import_date, "Earliest import date for query");

        let torrents = self.seedbox.query_torrents(earliest_import_date).await?;
        tracing::debug!(
            count = torrents.len(),
            "Received torrent updates from seedbox"
        );

        self.store.update_torrent_info(&torrents).await?;
        tracing::info!("Updated torrent information in storage");

        Ok(())
    }

    #[instrument(skip(self))]
    #[allow(clippy::needless_pass_by_value)] // We might carry payload in the future.
    pub fn handle_command(self: Arc<Self>, cmd: BotCommand) {
        match cmd {
            BotCommand::PullJob => {
                tracing::info!(command = "pull", "User requested pull job via bot");
                tokio::spawn(async move {
                    self.run_pull_job().await;
                });
            }
            BotCommand::TransferJob => {
                tracing::info!(command = "transfer", "User requested transfer job via bot");
                tokio::spawn(async move {
                    self.run_transfer_job().await;
                });
            }
            BotCommand::ReportAvailable => {
                tracing::info!(
                    command = "report",
                    "User requested report of available torrents/artifacts via bot"
                );
                tokio::spawn(async move {
                    self.run_report_job().await;
                });
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AniplerDaemonError {
    #[error("QBit API responded with an invalid response: {0}")]
    InvalidQBitApiResponse(String),
    #[error("rsync transfer failed: {0}")]
    RsyncTransfer(#[from] RsyncTransmitterError),
    #[error("Storage manager error: {0}")]
    Storage(#[from] StorageManagerError),
}
