use std::sync::Arc;

use tokio::task::JoinHandle;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::{
    api::ApiServer,
    bot::{BotCommand, TelegramBot},
    config::DaemonConfig,
    qbit::QBitSeedbox,
    rsync::{RsyncTransmitter, RsyncTransmitterError},
    storage::{StorageManager, StorageManagerError},
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
    pub async fn run_transfer_job(&self) {
        tracing::info!("Starting transfer of ready torrents");

        match self.transfer_ready_torrents().await {
            Ok(()) => {
                tracing::info!("Transfer job completed successfully");
            }
            Err(AniplerDaemonError::RsyncTransfer(RsyncTransmitterError::OverlappingTransfer)) => {
                tracing::warn!(
                    state = "overlapping_job",
                    "Another transfer job is already in progress, skipping"
                );
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to transfer ready torrents");
            }
        }
    }

    /// Transfer all torrents marked as ready from seedbox to artifact storage.
    ///
    /// # Errors
    ///
    /// Returns `OverlappingTransferJob` if another transfer job is in progress.
    #[instrument(skip(self))]
    async fn transfer_ready_torrents(&self) -> Result<(), AniplerDaemonError> {
        let transmitter = self.transmitter.try_session()?;

        let ready_torrents = self.store.list_ready_torrents().await?;
        let total_count = ready_torrents.len();
        tracing::info!(count = total_count, "Found ready torrents for transfer");

        let mut transferred = 0;
        for torrent in ready_torrents {
            let hash = &torrent.hash;
            tracing::info!(torrent = %torrent.name, hash = %hash, "Starting torrent transfer");

            let source = torrent.content_path;
            let dest = self
                .store
                .artifact_storage_path(hash)
                .to_string_lossy()
                .to_string();

            self.store.prepare_artifact_storage(hash).await?;
            transmitter.transfer(&source, &dest).await?;
            if !self.config.no_transfer {
                self.store.mark_artifact_ready(hash).await?;
            }
            transferred += 1;
            tracing::info!(
                progress = transferred,
                total = total_count,
                "Transferred torrent"
            );
        }

        tracing::info!(count = transferred, "Transferred all ready torrents");

        drop(transmitter);

        Ok(())
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
                    let result: anyhow::Result<()> = async {
                        let torrents = self.store.list_ready_torrents().await?;
                        let artifacts = self.store.list_ready_artifacts().await?;
                        self.bot.report_available(&torrents, &artifacts).await?;
                        Ok(())
                    }
                    .await;

                    if let Err(e) = result {
                        tracing::error!(error = ?e, "Failed to report available torrents/artifacts");
                    }
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
