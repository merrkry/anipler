use std::sync::Arc;

use tokio::task::JoinHandle;
use tokio_cron_scheduler::{JobScheduler, JobSchedulerError};

use crate::{config::DaemonConfig, qbit::QBitSeedbox, storage::StorageManager};

pub struct AniplerDaemon {
    config: DaemonConfig,
    seedbox: QBitSeedbox,
    store: StorageManager,
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

        let daemon = Self {
            config,
            seedbox,
            store,
        };

        Ok(Arc::new(daemon))
    }

    /// Run the main event loop of the daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if anything fails during starup, or an unrecoverable error occurs during runtime.
    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let jobs_handle = self.clone().run_jobs().await?;

        self.run_pull_job().await;

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::info!("Received Ctrl-C, shutting down");
            }
        };

        jobs_handle.await??;

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

        if !self.config.no_transfer {
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
        }

        let handle = tokio::spawn(async move { sched.start().await });

        Ok(handle)
    }

    /// Wrapper around pulling jobs with errors catched and logged.
    pub async fn run_pull_job(&self) {
        log::info!("Pulling torrents information from seedbox");

        self.update_status()
            .await
            .unwrap_or_else(|e| log::error!("Failed to pull torrents information: {e:?}"));
    }

    /// Wrapper around transfer jobs with errors catched and logged.
    pub async fn run_transfer_job(&self) {
        unimplemented!();
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
}
