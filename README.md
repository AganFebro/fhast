# fhast

fhast is a Rust CLI/TUI download manager with a daemon-owned downloader. The CLI talks to `fhast-daemon`; it does not download files directly.

## Quick Start

```bash
# Build
cargo build

# Start the daemon
cargo run -p fhast-cli -- daemon start

# Add a download
cargo run -p fhast-cli -- add "https://example.com/file.zip"

# Open the TUI dashboard
cargo run -p fhast-cli -- tui

# Open the Windows GUI dashboard
cargo run -p fhast-gui

# Stop the daemon
cargo run -p fhast-cli -- daemon stop
```

Or use the convenience build scripts:

```bash
./scripts/build.sh            # Full build (Rust + extension)
./scripts/build-rust.sh       # Rust only (build, test, clippy, fmt)
./scripts/build-extension.sh  # Extension only (typescript, lint, format)
```

## Commands

| Command | Description |
|---|---|
| `fhast daemon start` | Start the background daemon |
| `fhast daemon stop` | Stop the daemon gracefully |
| `fhast add <url>` | Queue a download |
| `fhast add <url> --out <path>` | Download to a specific path |
| `fhast add <url> --connections <n>` | Override segment count |
| `fhast list` | List all downloads |
| `fhast list --json` | List as JSON |
| `fhast info <id>` | Show download details |
| `fhast info <id> --json` | Show details as JSON |
| `fhast pause <id>` | Pause an active download |
| `fhast resume <id>` | Resume a paused download |
| `fhast remove <id>` | Remove a download and its files |
| `fhast retry <id>` | Retry a failed download |
| `fhast events` | Show recent daemon events |
| `fhast tui` | Open the terminal dashboard |
| `fhast config get [key]` | Show all or specific config |
| `fhast config set <key> <value>` | Set a config value |
| `fhast doctor` | Run system diagnostics |

The Windows GUI is available as `fhast-gui`. It starts `fhast-daemon` when needed, shows download progress and details, provides download controls and config editing, and minimizes to the system tray so downloads keep running.

## Chrome Extension

fhast includes a Chrome extension for capturing download links.

1. Load the extension from `extension/` at `chrome://extensions/` (Developer mode → Load unpacked)
2. Install the native host: `cargo run -p fhast-native-host -- --install-host`
3. Right-click a link → "Send link to fhast", or use the toolbar popup

See `docs/chrome-extension.md` for details.

## Documentation

| Doc | Covers |
|---|---|
| [architecture.md](docs/architecture.md) | System overview, crate map, data flow |
| [development.md](docs/development.md) | Setup, build, run, extension development |
| [ipc-protocol.md](docs/ipc-protocol.md) | IPC message schemas and transport |
| [download-engine.md](docs/download-engine.md) | Downloader modes, segments, resume, retry |
| [tui.md](docs/tui.md) | TUI views, keybindings, architecture |
| [chrome-extension.md](docs/chrome-extension.md) | Extension & native host development |
| [database.md](docs/database.md) | SQLite schema, migrations, statuses |
| [configuration.md](docs/configuration.md) | Environment variables, paths, defaults |
| [testing.md](docs/testing.md) | Test server modes, coverage, writing tests |
| [troubleshooting.md](docs/troubleshooting.md) | Common issues and fixes |
| [contributing.md](docs/contributing.md) | PR guidelines, code style, commit conventions |

When installed, the user-facing binary is `fhast` and the daemon binary is `fhast-daemon`.
