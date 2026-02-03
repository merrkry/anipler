# Anipler Design Document

## Goal

Automated torrent file transfer system with Telegram bot control.

## Architecture

Three machines in a relay chain:

- **Seedbox**: Runs qBittorrent, downloads torrents (VPS)
- **Relay**: Runs daemon with SQLite state, receives files via rsync over SSH (Mini PC)
- **Puller**: Desktop CLI to transfer files from Relay to local storage

## Transfer Flow

1. Seedbox downloads torrent to completion
2. At scheduled time, daemon rsyncs completed files from Seedbox to Relay
3. Daemon marks tasks as "available" in SQLite
4. Telegram bot notifies user via `/report` command
5. User runs `anipler-pull` on Puller
6. Files rsync to Puller's local storage
7. Files deleted from Relay atomically

## Components

### Daemon (anipler-daemon)

Coordinates all operations:
- Polls Telegram for commands
- Queries qBittorrent API for torrent status
- Runs cron jobs for pull and transfer operations
- Manages SQLite state persistence
- Executes rsync over SSH for file transfers

### Telegram Bot

Commands:
- `/pull`: Query seedbox and update local state
- `/transfer`: Rsync ready torrents to relay storage
- `/report`: List available torrents and artifacts

Long polling with exponential backoff (1s → 60s max) on errors. Only accepts commands from configured chat_id.

### Storage

SQLite database tracks:
- Torrent info (hash, name, status, content path)
- Artifact readiness status
- Import dates for incremental queries

## Environment

- Single-user, configured via environment variables
- SQLite for state persistence on Relay
- SSH key authentication for rsync transfers

## Module Structure

```
src/
├── lib.rs              Module declarations
├── model.rs            Data types
├── config.rs           Configuration from env vars
├── error.rs            Error types
├── daemon.rs           Main coordinator
├── api.rs              HTTP API server for puller
├── bot.rs              Telegram bot
├── qbit.rs             qBittorrent API client
├── rsync.rs            rsync over SSH wrapper
├── storage.rs          SQLite persistence
├── puller.rs           Puller library (client, config, transfer)
└── task.rs             Task types

src/bin/
├── daemon.rs           Daemon entry point
└── puller.rs           Puller CLI entry point
```

## Implementation Status

- **Daemon (anipler-daemon)**: Fully implemented, includes HTTP API server for puller
- **Puller (anipler-pull)**: Fully implemented CLI that fetches and transfers artifacts
- **All core modules**: Implemented and integrated

## Development History

- **Recent**: Implemented puller CLI and API server for daemon
- **Features implemented**: Puller config with shell expansion, HTTP API for artifact listing/confirmation, rsync transfer over SSH, atomic deletion confirmation
- **Status**: Both daemon and puller fully operational

## Tech Stack

- **Language**: Rust
- **qBittorrent API**: qbit-rs crate
- **Telegram Bot**: frankenstein crate (async)
- **Database**: SQLite with sqlx
- **Transfer**: rsync over SSH subprocess
- **Scheduling**: tokio-cron-scheduler
- **Async Runtime**: tokio
- **Logging**: tracing + tracing-subscriber (structured logging)
