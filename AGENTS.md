# AGENTS.md

## Goal

Automated torrent file transfer system with Telegram bot control.

## Design

We consider 3 machines in the relay chain:

- **Seedbox**: Runs qBittorrent, downloads torrents (VPS)
- **Relay**: Runs daemon with SQLite state, receives files via rsync over SSH
- **Puller**: Desktop CLI to transfer files from Relay to local storage

### Transfer Flow

1. Seedbox downloads torrent to completion.
2. At scheduled time, daemon rsyncs completed files from Seedbox to Relay.
3. Daemon marks tasks as "available" in SQLite, and notifies user.
4. User runs `anipler-pull` on Puller.
5. Files rsync to Puller's local storage and are deleted from Relay automatically.
