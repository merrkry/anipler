# anipler

Automated torrent file transfer system with Telegram bot control.

## Build

Use system libraries:

```
cargo build
```

### Glibc

For reproducible packaging:

```
nix build .#default
```

### Musl

Use `zigbuild` to build statically linked binaries, e.g.

```
cargo zigbuild --target x86_64-unknown-linux-musl --release
```
