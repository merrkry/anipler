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

### Completed Components

| Module | Status | Description |
|--------|--------|-------------|
| `daemon.rs` | Complete | Main coordinator with cron scheduling, command handling |
| `api.rs` | Complete | HTTP API server with Bearer token auth |
| `bot.rs` | Complete | Telegram bot with long polling and commands |
| `storage.rs` | Complete | SQLite persistence with full CRUD |
| `rsync.rs` | Complete | rsync wrapper with SSH, speed limiting |
| `puller.rs` | Complete | CLI library with shell expansion, transfers |
| `config.rs` | Complete | Environment variable configuration |
| `model.rs` | Complete | Data structures |
| `task.rs` | Complete | Task status structures |
| `error.rs` | Complete | Custom error types |

### Incomplete Components

| Module | Status | Description |
|--------|--------|-------------|
| `qbit.rs` | **Partial** | `upload_torrent()` is unimplemented |

**Missing Feature**: `qbit::Client::upload_torrent()` at `src/qbit.rs:27` - Upload torrent files to seedbox and tag with "anipler" for tracking.

## Development History

- **Recent**: Implemented puller CLI and API server for daemon
- **Features implemented**: Puller config with shell expansion, HTTP API for artifact listing/confirmation, rsync transfer over SSH, atomic deletion confirmation
- **Status**: Core transfer flow operational; torrent upload pending implementation

## Tech Stack

- **Language**: Rust
- **qBittorrent API**: qbit-rs crate
- **Telegram Bot**: frankenstein crate (async)
- **Database**: SQLite with sqlx
- **Transfer**: rsync over SSH subprocess
- **Scheduling**: tokio-cron-scheduler
- **Async Runtime**: tokio
- **Logging**: tracing + tracing-subscriber (structured logging)
