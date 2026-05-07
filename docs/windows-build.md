# fhast — Windows GUI Application Build

## What This Is

fhast is a download manager. On Linux it runs as a CLI/TUI terminal app. On Windows, we want a **full native GUI desktop application** — no terminal, no command line, just a normal Windows window with buttons, progress bars, and download management.

The Linux version is fully built and tested. Your job is to create the **Windows GUI frontend** that wraps the existing download engine.

## What Already Works (Cross-Platform)

The project was originally Linux-only. We recently added cross-platform support to the core layers. Here's what's done and verified on Linux (30 tests passing, clippy clean):

### Download Engine (`crates/fhast-core/`)
The most important crate — handles all HTTP download logic. **Completely cross-platform, zero changes needed for Windows.**
- Single-connection and segmented (multi-part) downloads
- Resume with ETag/Last-Modified validation
- Retry with exponential backoff for transient errors (timeouts, DNS, connection resets, 5xx)
- HTTP 429 rate limit handling with Retry-After parsing
- Checksum verification (SHA-256, MD5)
- SQLite database for persistent state (jobs, segments, headers, events)
- Cookie/header forwarding from browser captures
- Config file read/write (`max_retries`, connection limits, output dir, privacy settings)

### IPC Transport (`crates/fhast-ipc/` + `crates/fhast-ipc/src/connection.rs`)
All inter-process communication between daemon and clients. **Already cross-platform via the `interprocess` crate.**
- On Linux/macOS: Unix domain sockets
- On Windows: **named pipes** (`\\.\pipe\fhast-<user>`)
- Transport layer: newline-delimited JSON — platform agnostic
- `connect_to_daemon()` and `bind_daemon_socket()` work on all platforms with identical API
- 10 IPC request types: AddDownload, ListDownloads, DownloadInfo, PauseDownload, ResumeDownload, RemoveDownload, RetryDownload, RecentEvents, DownloadSegments, DownloadHeaders
- IPC is request-reply: client sends JSON, daemon responds with JSON

### Daemon (`crates/fhast-daemon/`)
The background process that owns all downloads. **Already cross-platform.**
- Starts, listens on named pipe (Windows) or Unix socket (Linux/macOS)
- `fhast daemon start` / `fhast daemon stop` commands
- Handles all IPC requests, manages download queue, schedules workers
- Daemon state: storage, downloader, active downloads map, speed tracker
- On Linux, the CLI/TUI connect as clients. On Windows, the GUI will connect the same way.

### Path Resolution (`crates/fhast-core/src/paths.rs`)
**Already has per-platform `#[cfg]` blocks for all three OS.**
- Linux: `$XDG_RUNTIME_DIR/fhast/`, `~/.local/state/fhast/`, `~/.config/fhast/`
- macOS: `/tmp/fhast-<uid>/`, `~/Library/Application Support/fhast/`, `~/.config/fhast/`
- Windows: `%LOCALAPPDATA%\fhast\`, `\\.\pipe\fhast-<user>`, `%APPDATA%\fhast\`

### Chrome Extension (`extension/`)
TypeScript Manifest V3 extension. **Works identically on all platforms.**
- Auto-grab: intercepts browser downloads, captures cookies/headers, forwards to daemon
- Redirect chain tracking: cookies follow 302 redirects to CDN URLs
- File detection: URL extension matching, Content-Disposition parsing, MIME type detection
- Popup UI with auto-grab toggle and capture history

### Chrome Native Host (`crates/fhast-native-host/`)
Bridges Chrome extension to daemon via stdin/stdout JSON framing. **Already cross-platform.**
- Has per-platform Chrome user data directory resolution
- Linux: `~/.config/google-chrome/NativeMessagingHosts/`
- macOS: `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`
- Windows: `%LOCALAPPDATA%\Google\Chrome\User Data\NativeMessagingHosts\`

## What You Need to Build

### The Windows GUI App

Create a new crate: `crates/fhast-gui/`. This replaces `fhast-cli` and `fhast-tui` on Windows with a single graphical application.

**Requirements:**
- Single `.exe` file — portable, no installer needed
- Full GUI with window, menus, buttons (not terminal/TUI)
- Native Windows look and feel
- Bundles or auto-starts `fhast-daemon.exe` in the background
- Communicates with daemon via the existing named pipe IPC

**GUI Frameworks to Consider:**
- **egui** (via `eframe` or `egui-winit` + `egui-wgpu`) — immediate mode, pure Rust, simple
- **iced** — Elm-like architecture, pure Rust, more structured
- **slint** — declarative UI language, native look, .slint files
- **dioxus** — React-like, uses webview or native renderer
- **native-windows-gui** — wraps Win32 API directly, most native look

Recommendation: **egui** with `eframe` is the fastest path to a working GUI. It's mature, well-documented, and doesn't require learning a new DSL. Use `eframe::NativeOptions` with `native_window_appearance: Dark` for a modern look.

**What the GUI Needs to Display:**

```
┌─────────────────────────────────────────────────────────┐
│ fhast                                          ─ □ ✕    │
├─────────────────────────────────────────────────────────┤
│ [+ Add URL...]  [Config]  [Doctor]                       │
├─────────────────────────────────────────────────────────┤
│ ████████████░░░░░░░░  file.zip  5.2/100 MB  [⏸] [✕]    │
│ ░░░░░░░░░░░░░░░░░░░░  video.mp4  Queued        [✕]      │
│ ✅ document.pdf → C:\Downloads\document.pdf              │
│ ❌ broken.bin  (connection reset)              [↻]       │
├─────────────────────────────────────────────────────────┤
│ Details                                                │
│  URL: https://cdn.example.com/file.zip                  │
│  Save to: C:\Users\...\Downloads\file.zip               │
│  Status: Downloading · 52% · 2.1 MB/s                   │
│  Connections: 8  ·  Retries: 0                          │
│  Segments: [████][████][░░░░][░░░░]                      │
│  Headers: 13 (+1 cookie)   Response: Content-Length:... │
├─────────────────────────────────────────────────────────┤
│ Status: Connected to daemon · 3 active downloads        │
└─────────────────────────────────────────────────────────┘
```

**Features to Implement:**
1. Download list with progress bars, speed, status
2. Add URL dialog (paste URL, optional output path)
3. Pause/Resume/Remove/Retry buttons per download
4. Detail panel showing URL, save path, segments map, headers
5. Config panel (max retries, connections, output dir)
6. System tray icon (minimize to tray, right-click menu: Show/Exit)
7. Auto-start daemon on app launch, stop on app close
8. "Auto-grab status" indicator showing if Chrome extension is connected
9. Recent events log
10. Drag-and-drop URL/file onto window to add download

**Architecture:**
```
fhast-gui.exe (GUI window)
    │
    │ spawns on startup
    ▼
fhast-daemon.exe (background, hidden window or no window)
    │
    │ named pipe IPC (same as Linux, already coded)
    ▼
All download logic (FHast-core) — unchanged from Linux
```

The GUI spawns `fhast-daemon.exe` as a child process on startup (hidden, no console window), connects via named pipe, and polls every 500ms for download list updates (same polling pattern as `fhast-tui` on Linux).

**IPC Integration:**

All IPC is already defined — you just call the same Rust functions the CLI uses:

```rust
use fhast_ipc::{
    connect_to_daemon, read_message, split_stream, write_message,
    IpcRequest, IpcResponse, DownloadDto, SegmentDto, EventDto, IPC_VERSION,
};

// Connect to the running daemon
let socket_path = fhast_core::paths::socket_path()?;
let stream = connect_to_daemon(&socket_path).await?;
let (reader, mut writer) = split_stream(stream);

// Send a request
let request = IpcRequest::AddDownload {
    version: IPC_VERSION,
    url: "https://example.com/file.zip".to_string(),
    output_path: None,
    working_dir: std::env::current_dir()?,
    connections: None,
    checksum_sha256: None,
    checksum_md5: None,
    headers: None,
    sensitive_headers: None,
};
write_message(&mut writer, &request).await?;

// Read response
let response: IpcResponse = read_message(&mut reader).await?;
```

See `docs/ipc-protocol.md` for ALL available request/response types with JSON schemas.

## What NOT to Change

- **`crates/fhast-core/`** — download engine, storage, models. Completely cross-platform, thoroughly tested.
- **`crates/fhast-ipc/`** — message types, transport, connection. Already handles Windows named pipes.
- **`crates/fhast-daemon/`** — daemon process. Already cross-platform. The GUI just needs to spawn and stop it.
- **`crates/fhast-native-host/`** — Chrome bridge. Already has Windows paths.
- **`extension/`** — Chrome extension. Works identically on all platforms.
- **`crates/fhast-cli/`** and **`crates/fhast-tui/`** — existing terminal apps. Keep them for Linux. Don't modify.

## Key Dependencies

```toml
[workspace.dependencies]
# Already in the project:
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["stream", "rustls-tls"] }
interprocess = { version = "2", features = ["tokio"] }  # Handles named pipes on Windows
rusqlite = { version = "0.32", features = ["bundled"] }  # Compiles SQLite from source
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"

# Add for GUI (recommendation: egui):
eframe = "0.31"
egui = "0.31"
```

## Workspace Setup

Add to root `Cargo.toml`:
```toml
[workspace]
members = [
  # ... existing crates ...
  "crates/fhast-gui",
]
```

Create `crates/fhast-gui/Cargo.toml`:
```toml
[package]
name = "fhast-gui"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
anyhow.workspace = true
fhast-core = { path = "../fhast-core" }
fhast-ipc = { path = "../fhast-ipc" }
tokio.workspace = true
eframe = "0.31"
egui = "0.31"
```

## Testing Checklist

- [x] GUI window opens with title "fhast"
- [x] Daemon auto-starts when GUI opens (hidden, no console)
- [x] Download list shows active/queued/completed/failed downloads
- [x] Add URL via button/dialog queues a download
- [x] Progress bars update in real-time (500ms polling)
- [x] Pause/Resume/Remove/Retry buttons work via IPC
- [x] Detail panel shows URL, path, headers, segment map
- [x] Config panel reads/writes `%APPDATA%\fhast\config.json`
- [x] System tray: minimize to tray, right-click context menu
- [x] Closing the window minimizes to tray; tray/in-app Exit stops the daemon only when the GUI started it
- [x] Chrome extension auto-grab works with Windows native host
- [x] State files go to `%LOCALAPPDATA%\fhast\` (not `/run/user/` or Unix paths)

## Reference Docs

Read these for full context (in `docs/`):
- **`docs/architecture.md`** — system overview, crate map, data flow (critical read)
- **`docs/ipc-protocol.md`** — ALL IPC message schemas with JSON examples (critical read)
- `docs/download-engine.md` — downloader internals, segment math, retry policy
- `docs/configuration.md` — config keys, env vars, paths per platform
- `docs/database.md` — SQLite schema (downloads, segments, headers, events tables)
- `docs/chrome-extension.md` — extension + native host interaction
- `docs/testing.md` — existing test coverage, test server modes
- `docs/development.md` — build commands, workspace structure

## Summary

You're building a native Windows GUI that wraps an already-working download engine. The daemon, IPC, paths, error handling, Chrome integration, and download logic are all done and tested. Your job is purely the **UI layer** — create a graphical window that talks to the existing daemon over named pipes using the existing IPC protocol. Don't change the engine, don't change the protocol, don't change the daemon.
