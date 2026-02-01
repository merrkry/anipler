use std::{cmp::min, fmt::Write, sync::Arc, time::Duration};

use frankenstein::{
    AsyncTelegramApi,
    methods::{GetUpdatesParams, SendMessageParams, SetMyCommandsParams},
    types::{BotCommandScope, BotCommandScopeChat, Message},
    updates::UpdateContent,
};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    config::DaemonConfig,
    task::{ArtifactInfo, TorrentTaskInfo},
};

pub enum BotCommand {
    PullJob,
    TransferJob,
    ReportAvailable,
}

impl BotCommand {
    fn parse(input: &str) -> Option<Self> {
        let body = input.strip_prefix('/')?;
        let (cmd, _body) = match body.split_once(' ') {
            Some((c, b)) => (c, b),
            None => (body, ""),
        };

        match cmd {
            "pull" => Some(Self::PullJob),
            "transfer" => Some(Self::TransferJob),
            "report" => Some(Self::ReportAvailable),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum TelegramBotError {
    #[error("bot not running")]
    NotRunning,
    #[error("command channel closed")]
    ChannelClosed,
    #[error("shutdown failed: {0}")]
    ShutdownFailed(anyhow::Error),
}

struct State {
    handle: JoinHandle<()>,
    cancel: CancellationToken,
}

pub struct TelegramBot {
    bot: Arc<frankenstein::client_reqwest::Bot>,
    chat_id: i64,
    rx: Mutex<Option<mpsc::Receiver<BotCommand>>>,
    state: Mutex<Option<State>>,
}

impl TelegramBot {
    /// Create a new Telegram bot instance from the given configuration.
    pub fn from_config(config: &DaemonConfig) -> Self {
        let bot = Arc::new(frankenstein::client_reqwest::Bot::new(
            &config.telegram_bot_token,
        ));
        let chat_id = config.telegram_chat_id;
        Self {
            bot,
            chat_id,
            rx: Mutex::new(None),
            state: Mutex::new(None),
        }
    }

    /// Run the main event loop of the Telegram bot in background task.
    pub async fn run(&self) -> anyhow::Result<()> {
        self.register_commands().await?;

        let cancel = CancellationToken::new();
        let (tx, rx) = mpsc::channel(16);

        let handle = {
            let bot = self.bot.clone();
            let chat_id = self.chat_id;
            let cancel = cancel.clone();

            let mut update_params = GetUpdatesParams::builder()
                .allowed_updates(vec![frankenstein::types::AllowedUpdate::Message])
                .build();

            tokio::spawn(async move {
                const MIN_RETRY_DELAY: Duration = Duration::from_secs(1);
                const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
                let mut retry_delay = MIN_RETRY_DELAY;

                loop {
                    let updates;

                    tokio::select! {
                        () = cancel.cancelled() => break,
                        updates_result = bot.get_updates(&update_params) => {
                            updates = updates_result;
                        }
                    };

                    let Ok(updates) = updates else {
                        log::error!("Failed to get updates from Telegram");
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = min(retry_delay * 2, MAX_RETRY_DELAY);
                        continue;
                    };

                    retry_delay = MIN_RETRY_DELAY;

                    assert!(updates.ok); // always true accordingly to frankenstein documentation.

                    for update in updates.result {
                        let UpdateContent::Message(message) = &update.content else {
                            continue;
                        };

                        if message.chat.id != chat_id {
                            continue;
                        }

                        Self::handle_message(message, &tx).await;

                        update_params.offset = Some(i64::from(update.update_id) + 1);
                    }
                }

                log::debug!("Telegram bot main loop exited");
            })
        };

        *self.state.lock().await = Some(State { handle, cancel });
        *self.rx.lock().await = Some(rx);

        Ok(())
    }

    pub async fn recv_command(&self) -> Result<BotCommand, TelegramBotError> {
        self.rx
            .lock()
            .await
            .as_mut()
            .ok_or(TelegramBotError::NotRunning)?
            .recv()
            .await
            .ok_or(TelegramBotError::ChannelClosed)
    }

    pub async fn shutdown(&self) -> Result<(), TelegramBotError> {
        let state = self
            .state
            .lock()
            .await
            .take()
            .ok_or(TelegramBotError::NotRunning)?;

        self.rx
            .lock()
            .await
            .take()
            // SAFETY: `rx` should be filled in `run()` and be taken here,
            // keeping the same state as `state`.
            .expect("rx should be available when bot is running");

        state.cancel.cancel();
        state
            .handle
            .await
            .map_err(|e| TelegramBotError::ShutdownFailed(e.into()))
    }

    async fn register_commands(&self) -> anyhow::Result<()> {
        let pull_command = frankenstein::types::BotCommand::builder()
            .command("pull")
            .description("Pull torrents information from seedbox.")
            .build();

        let transfer_command = frankenstein::types::BotCommand::builder()
            .command("transfer")
            .description("Transfer torrents from seedbox to relay.")
            .build();

        let report_command = frankenstein::types::BotCommand::builder()
            .command("report")
            .description("Report available torrents and artifacts.")
            .build();

        let params = SetMyCommandsParams::builder()
            .commands(vec![pull_command, transfer_command, report_command])
            .scope(BotCommandScope::Chat(BotCommandScopeChat {
                chat_id: self.chat_id.into(),
            }))
            .build();

        self.bot.set_my_commands(&params).await?;

        Ok(())
    }

    async fn handle_message(msg: &Message, rx: &mpsc::Sender<BotCommand>) {
        let Some(text) = &msg.text else {
            log::info!("Received empty message");
            return;
        };

        let cmd = BotCommand::parse(text);

        let Some(cmd) = cmd else {
            log::info!("Received invalid command: {text}");
            return;
        };

        match rx.send(cmd).await {
            Ok(()) => {
                log::info!("Received command: {text}");
            }
            Err(e) => {
                log::warn!("Failed to send command to handler: {e}");
            }
        }
    }

    /// Report available torrents and artifacts.
    pub async fn report_available(
        &self,
        torrents: &[TorrentTaskInfo],
        artifacts: &[ArtifactInfo],
    ) -> anyhow::Result<()> {
        let mut text = String::new();

        if !torrents.is_empty() {
            writeln!(text, "Ready Torrents:")?;
            for torrent in torrents {
                writeln!(text, "\n- {} \n  ({})", torrent.name, torrent.hash)?;
            }
        }

        if !artifacts.is_empty() {
            if !text.is_empty() {
                writeln!(text)?;
            }
            writeln!(text, "Available Artifacts:")?;
            for artifact in artifacts {
                writeln!(text, "\n- {} \n  ({})", artifact.name, artifact.hash)?;
            }
        }

        if text.is_empty() {
            text = "No torrents or artifacts available".to_string();
        }

        let params = SendMessageParams::builder()
            .chat_id(self.chat_id)
            .text(text)
            .build();
        self.bot.send_message(&params).await?;
        Ok(())
    }
}
