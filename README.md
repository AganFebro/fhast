# fhast

fhast is a Rust download manager with a Windows desktop app, a terminal UI, and a command-line interface. On Windows, most users should open the GUI app and let it manage the background helper programs automatically.

## Quick Start - Windows

Build or unpack a Windows release, then open the folder that contains these files:

```text
fhast-gui.exe
fhast-daemon.exe
fhast-native-host.exe
```

Double-click `fhast-gui.exe` to start fhast.

Simple Windows flow:

1. Double-click `fhast-gui.exe`.
2. Click **Add URL** and paste a download link.
3. Watch progress in the download table.
4. Double-click a file row to see details.
5. Press **X** and choose **Minimize to Tray** to keep downloads running, or **Close fhast** to exit.

Keep `fhast-daemon.exe` and `fhast-native-host.exe` in the same folder as `fhast-gui.exe`. They are helper programs used by the GUI; users do not need to open them manually.

If you built from source with Cargo, create the release executables with:

```powershell
cargo build --release
```

Then open `target\release\` in File Explorer and double-click `fhast-gui.exe`.

For browser integration, load the Chrome extension, then open **Extension** in the GUI and click **Register Chrome Integration (Native Host)**. This registers the `fhast-native-host.exe` helper that Chrome uses to talk to fhast. See [windows-gui.md](docs/windows-gui.md) for the full Windows guide.

## Quick Start - Terminal

```bash
# Build
cargo build

# Start the daemon
cargo run -p fhast-cli -- daemon start

# Add a download
cargo run -p fhast-cli -- add "https://example.com/file.zip"

# Open the TUI dashboard
cargo run -p fhast-cli -- tui

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

The Windows GUI is available as `fhast-gui.exe`. It starts its helper service when needed, shows downloads in a responsive resizable table, opens details on double-click, provides download controls and config editing, and lets users choose between minimizing to the tray or closing fhast when pressing **X**.

## Chrome Extension

fhast includes a Chrome extension for capturing download links.

1. Load the extension from `extension/` at `chrome://extensions/` (Developer mode → Load unpacked)
2. Open **Extension** in `fhast-gui.exe`, paste the Chrome extension ID, and click **Register Chrome Integration (Native Host)**
3. Right-click a link → "Send link to fhast", or use the toolbar popup

See `docs/chrome-extension.md` for details.

## Documentation

| Doc | Covers |
|---|---|
| [architecture.md](docs/architecture.md) | System overview, crate map, data flow |
| [development.md](docs/development.md) | Setup, build, run, extension development |
| [windows-gui.md](docs/windows-gui.md) | Windows desktop app usage, tray, daemon, extension setup |
| [windows-gui-architecture.md](docs/windows-gui-architecture.md) | Windows GUI process model, IPC, native-host integration |
| [ipc-protocol.md](docs/ipc-protocol.md) | IPC message schemas and transport |
| [download-engine.md](docs/download-engine.md) | Downloader modes, segments, resume, retry |
| [tui.md](docs/tui.md) | TUI views, keybindings, architecture |
| [chrome-extension.md](docs/chrome-extension.md) | Extension & native host development |
| [database.md](docs/database.md) | SQLite schema, migrations, statuses |
| [configuration.md](docs/configuration.md) | Environment variables, paths, defaults |
| [testing.md](docs/testing.md) | Test server modes, coverage, writing tests |
| [troubleshooting.md](docs/troubleshooting.md) | Common issues and fixes |
| [contributing.md](docs/contributing.md) | PR guidelines, code style, commit conventions |

On Windows, the user-facing app is `fhast-gui.exe`. `fhast-daemon.exe` and `fhast-native-host.exe` are helper programs that should stay beside it.
