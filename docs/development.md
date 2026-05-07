# Development Setup

## Prerequisites

- **Rust** 1.80+ (stable)
- **Node.js** 20+ (for Chrome extension TypeScript build)
- **Chrome** or Chromium (for extension testing)
- **Linux**, **macOS**, or **Windows** (cross-platform)

## Clone & Build

```bash
git clone <repo-url> fhast
cd fhast
cargo build
```

This builds all crates: `fhast-core`, `fhast-ipc`, `fhast-daemon`, `fhast-cli`, `fhast-tui`, `fhast-gui`, `fhast-native-host`.

## Run Tests

```bash
cargo test
```

This runs all Rust unit and integration tests (currently 26 tests across all crates).

## Code Quality

```bash
cargo fmt                                          # Format all Rust code
cargo clippy --all-targets --all-features -- -D warnings  # Lint all Rust code
```

For the Chrome extension:

```bash
cd extension
npm install
npm run lint      # TypeScript type check
npm run format    # Prettier formatting
npm run build     # Compile TypeScript to dist/
```

## Running Locally

Start the daemon:

```bash
cargo run -p fhast-cli -- daemon start
```

Add a download:

```bash
cargo run -p fhast-cli -- add "https://example.com/file.zip"
```

Check progress in the TUI:

```bash
cargo run -p fhast-cli -- tui
```

Run the Windows desktop GUI:

```bash
cargo run -p fhast-gui
```

The GUI starts `fhast-daemon` when it is not already running. Closing the window opens a prompt: minimize to the tray to keep downloads running, or close fhast and ask the daemon to stop. The tray `Exit` command and in-app `Exit` button close fhast and ask the daemon to stop.

Stop the daemon:

```bash
cargo run -p fhast-cli -- daemon stop
```

## Chrome Extension Development

The extension lives in `extension/` with this structure:

```
extension/
  manifest.json       # Manifest V3 config
  *.ts                # TypeScript source
  dist/               # Compiled JS output (gitignored)
  icons/              # Extension icons (PNG)
  popup.html          # Toolbar popup HTML
```

After making TypeScript changes, rebuild:

```bash
cd extension && npm run build
```

Then reload the extension at `chrome://extensions/` by clicking the refresh icon.

### Installing the Native Host

In the Windows GUI, this appears as **Register Chrome Integration (Native Host)** so users can connect the browser setup step to the `fhast-native-host.exe` helper.

The native host bridge requires a manifest at:

```
~/.config/google-chrome/NativeMessagingHosts/fhast_native_host.json
```

Generate and install it:

```bash
# First load the extension in Chrome to get its ID from chrome://extensions/
export fhast_EXTENSION_ID="your-extension-id-here"

# Install (needs to run each time the extension ID changes)
cargo run -p fhast-native-host -- --install-host
```

Or install manually:

```bash
cargo run -p fhast-native-host -- --manifest  # Print the manifest
# Copy the output to ~/.config/google-chrome/NativeMessagingHosts/fhast_native_host.json
# Replace the placeholder extension ID with your actual one
```

## Environment Variables

| Variable | Purpose |
|---|---|
| `fhast_EXTENSION_ID` | Chrome extension ID for native host manifest generation |
| `RUST_LOG` | Tracing log level (e.g., `info`, `debug`, `fhast_daemon=debug`) |
| `XDG_RUNTIME_DIR` | Runtime directory for daemon socket (defaults to `/run/user/<uid>`) |
| `XDG_STATE_HOME` | State directory for SQLite DB and job files (defaults to `~/.local/state`) |

## Directory Layout

```
fhast/
  Cargo.toml              # Workspace root
  crates/
    fhast-core/           # Download engine, storage, models, tests
      src/
        downloader.rs     # HTTP download logic
        storage.rs        # SQLite storage layer
        model.rs          # DownloadRecord, Segment, DownloadStatus
        filename.rs       # Path sanitization, temp file naming
        paths.rs          # Platform-specific path resolution
        error.rs          # Error types
      tests/
        test_harness/     # Test helpers and embedded test server
        downloader_integration.rs  # Integration tests
    fhast-ipc/            # IPC message types and transport
      src/
        message.rs        # Request/response enums, DTOs
        transport.rs      # newline-delimited socket read/write
        error.rs
    fhast-daemon/         # Background daemon process
      src/
        main.rs           # Socket listener, state, handlers, workers
    fhast-cli/            # Command-line interface
      src/
        main.rs           # clap commands, IPC client
    fhast-tui/            # Terminal dashboard
      src/
        main.rs           # Ratatui app, views, keybindings
    fhast-gui/            # Windows desktop dashboard
      src/
        main.rs           # egui app, windows, controls
        daemon.rs         # daemon startup/shutdown management
        ipc_client.rs     # async IPC client helpers
        tray.rs           # system tray integration
    fhast-native-host/    # Chrome Native Messaging bridge
      src/
        main.rs           # stdin/stdout framing, message forwarding
  extension/              # Chrome extension
    manifest.json
    *.ts                  # TypeScript source
    dist/                 # Compiled output
    popup.html
    icons/
  docs/                   # Documentation
```
