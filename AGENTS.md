# Anipler Design Document

## Goal

Automated torrent file transfer system with Telegram bot control.

## Architecture

- **Seedbox**: Runs qBittorrent, downloads torrents (VPS)
- **Relay**: Runs daemon with SQLite state, receives files via rsync over SSH (Mini PC)
- **Puller**: Desktop CLI to transfer files from Relay to local storage

## Transfer Flow

1. Seedbox downloads torrent to completion
2. At scheduled time, daemon rsyncs completed files from Seedbox to Relay
3. Daemon marks tasks as "available" in SQLite
4. Telegram bot notifies user
5. User runs `anipler-pull` on Puller
6. Files rsync to Puller's local storage
7. Files deleted from Relay atomically

## Components

- `anipler-daemon`: Main daemon (qbit API + rsync + Telegram bot + SQLite)
- `anipler-pull`: Desktop CLI to pull files from Relay

## Tech Stack

- **Language**: Rust
- **qBittorrent API**: `qbit-rs` crate
- **Telegram Bot**: `frankenstein` crate
- **Database**: SQLite with `sqlx`
- **Transfer**: `rsync` over SSH

## Environment

- Single-user, env vars for credentials
- SQLite for state persistence on Relay

## Current Priorities

- Seedbox → Relay download via rsync over SSH
- Telegram: `add` command, `status` command, download notifications
- Desktop pull command with atomic transfer + cleanup

---

## Module Structure (Implemented)

```
src/
├── lib.rs              # Module declarations
├── model.rs            # Data types (TorrentSource re-export)
├── config.rs           # DaemonConfig (env var loading)
├── error.rs            # Error types (AniplerError)
├── daemon.rs           # AniplerDaemon (main coordinator)
├── qbit.rs             # QBitSeedbox (qBittorrent operations)
├── storage.rs          # StorageManager (SQLite persistence)
└── task.rs             # Task types (TorrentStatus, TorrentTaskInfo, ArtifactInfo)

src/bin/
├── daemon.rs           # Daemon entry point
└── pull.rs             # Puller CLI entry point (stub)
```

## Data Flow: Torrent & Artifact Handling

### Core Types

```rust
enum TorrentStatus { Downloading, Seeding }

struct TorrentTaskInfo {
    hash: String,
    status: TorrentStatus,
    content_path: String,
    name: String,
}

struct ArtifactInfo {
    hash: String,
}
```

### Data Lifecycle

1. **Add Torrent**: `QBitSeedbox.upload_torrent(TorrentSource)` -> `TorrentTaskInfo`
2. **Track State**: `StorageManager.update_torrent_info([TorrentTaskInfo])`
3. **Check Completion**: `StorageManager.list_ready_torrents()` -> transfer to relay
4. **Artifact Storage**: `prepare_artifact_storage(hash)` -> dir for file
5. **Archive Ready**: `mark_artifact_ready(hash)` -> `list_ready_artifacts()`
6. **Cleanup**: `reclaim_artifact_storage(hash)` -> delete after pull

## Implementation Status

| Component | Status | Notes |
|-----------|--------|-------|
| `config.rs` | Done | Env var loading, storage path setup |
| `daemon.rs` | Partial | `from_config`, `run`, `run_jobs`, `update_status` implemented; job handlers are stubs |
| `error.rs` | Done | `AniplerError::InvalidApiResponse` defined |
| `model.rs` | Done | `TorrentSource` re-export |
| `qbit.rs` | Partial | `query_torrents` implemented; `upload_torrent` is stub |
| `storage.rs` | Partial | `from_config` implemented; all other methods are stubs |
| `task.rs` | Done | Types defined |
