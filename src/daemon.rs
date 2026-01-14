use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};
use tokio_util::sync::CancellationToken;

use crate::{
    bot::{BotCommand, TelegramBot},
    config::DaemonConfig,
    error::AniplerError,
    qbit::QBitSeedbox,
    rsync::RsyncTransmitter,
    storage::StorageManager,
};

pub struct AniplerDaemon {
    bot: TelegramBot,
    config: DaemonConfig,
    seedbox: QBitSeedbox,
    store: StorageManager,
    transmitter: Mutex<RsyncTransmitter>,
}

impl AniplerDaemon {
    /// Create a new daemon instance from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization of any component fails.
    pub async fn from_config(config: DaemonConfig) -> anyhow::Result<Arc<Self>> {
        let seedbox = QBitSeedbox::from_config(&config);
        let store = StorageManager::from_config(&config).await?;

        let transmitter = RsyncTransmitter::from_config(&config);

        let bot = TelegramBot::from_config(&config);

        let daemon = Self {
            bot,
            config,
            seedbox,
            store,
            transmitter: Mutex::new(transmitter),
        };

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

        let mut bot_handle = self.bot.run(cancel.child_token()).await?;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    log::info!("Received Ctrl-C, shutting down");

                    cancel.cancel();

                    break;
                }
                cmd = bot_handle.rx.recv() => {
                    match cmd {
                        Some(cmd) => self.clone().handle_command(cmd).await,
                        None => {
                            log::error!("Telegram bot command channel closed unexpectedly, shutting down");
                        },
                    }
                }
            };
        }

        jobs_handle.await??;
        bot_handle.handle.await?;

        log::info!("Terminated gracefully");

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
    pub async fn run_pull_job(&self) {
        log::info!("Pulling torrents information from seedbox");

        self.update_status()
            .await
            .unwrap_or_else(|e| log::error!("Failed to pull torrents information: {e:?}"));
    }

    /// Wrapper around transfer jobs with errors caught and logged.
    pub async fn run_transfer_job(&self) {
        log::info!("Transferring ready torrents");

        match self.transfer_ready_torrents().await {
            Ok(()) => {
                log::info!("Transfer job completed successfully");
            }
            Err(e) => {
                if matches!(
                    e.downcast_ref::<AniplerError>(),
                    Some(AniplerError::OverlappingTransferJob)
                ) {
                    log::warn!("Another transfer job is already in progress, skipping");
                    return;
                }
                log::error!("Failed to transfer ready torrents: {e:?}");
            }
        }
    }

    /// Transfer all torrents marked as ready from seedbox to artifact storage.
    ///
    /// # Errors
    ///
    /// Returns `OverlappingTransferJob` if another transfer job is in progress.
    #[allow(clippy::significant_drop_tightening)] // we want to hold lock during the whole task
    async fn transfer_ready_torrents(&self) -> anyhow::Result<()> {
        // Transmitter lock is held during the whole transfer job,
        // because we rely on the status of the lock to determine if an existing job is in progress.
        let transmitter = self
            .transmitter
            .try_lock()
            // `TryLockError` "will only fail if the mutex is already locked"
            .map_err(|_: tokio::sync::TryLockError| AniplerError::OverlappingTransferJob)?;

        let ready_torrents = self.store.list_ready_torrents().await?;

        let len = ready_torrents.len();

        for torrent in ready_torrents {
            let hash = &torrent.hash;

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
        }

        log::info!("Transferred {len} torrents");

        Ok(())
    }

    /// Fetch the latest torrent status from the seedbox and update the local storage.
    ///
    /// # Errors
    ///
    /// Returns an error if querying the seedbox or updating the storage fails.
    pub async fn update_status(&self) -> anyhow::Result<()> {
        let earliest_import_date = self.store.earliest_import_date().await?;
        let torrents = self.seedbox.query_torrents(earliest_import_date).await?;
        self.store.update_torrent_info(&torrents).await?;
        Ok(())
    }

    pub async fn handle_command(self: Arc<Self>, cmd: BotCommand) {
        match cmd {
            BotCommand::PullJob => {
                log::info!("User requested pull job via bot");
                tokio::spawn(async move {
                    self.run_pull_job().await;
                });
            }
            BotCommand::TransferJob => {
                log::info!("User requested transfer job via bot");
                tokio::spawn(async move {
                    self.run_transfer_job().await;
                });
            }
            BotCommand::ReportAvailable => {
                let result: anyhow::Result<()> = async {
                    let torrents = self.store.list_ready_torrents().await?;
                    let artifacts = self.store.list_ready_artifacts().await?;
                    self.bot.report_available(&torrents, &artifacts).await?;
                    Ok(())
                }
                .await;

                if let Err(e) = result {
                    log::error!("Failed to report available torrents/artifacts: {e:?}");
                }
            }
        }
    }
}
