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
- **Database**: SQLite with `rusqlite`
- **Transfer**: `rsync` over SSH

## Environment

- Single-user, env vars for credentials
- SQLite for state persistence on Relay

## Current Priorities

- Seedbox → Relay download via rsync over SSH
- Telegram: `add` command, `status` command, download notifications
- Desktop pull command with atomic transfer + cleanup
