# Windows GUI Architecture

This document describes how `fhast-gui` fits into the fhast multi-process design.

## Process Model

```text
Chrome Extension
  |
  | Chrome Native Messaging
  v
fhast-native-host.exe
  |
  | fhast IPC
  v
fhast-daemon.exe <----- fhast-gui.exe
  |
  v
fhast-core
```

`fhast-daemon` is the only process that owns mutable download state. The GUI never writes download records directly and never performs HTTP downloads. It sends typed IPC requests and renders the responses.

## Crate Layout

| File | Role |
|---|---|
| `crates/fhast-gui/src/main.rs` | egui app state, windows, dialogs, worker messages, formatting helpers |
| `crates/fhast-gui/src/daemon.rs` | daemon discovery, hidden daemon startup, owned-daemon shutdown |
| `crates/fhast-gui/src/ipc_client.rs` | async IPC request helpers and GUI data snapshots |
| `crates/fhast-gui/src/tray.rs` | system tray icon, menu items, tray callbacks |

The GUI depends on `fhast-core` for config and paths, and `fhast-ipc` for request and response schemas.

## Startup

At startup, `DaemonManager::ensure_running()` resolves the Windows named pipe path through `fhast_core::paths::socket_path()`.

The startup flow is:

1. Try to connect to the daemon named pipe.
2. If connection succeeds, mark the daemon as externally owned.
3. If connection fails, resolve sibling `fhast-daemon.exe`.
4. Spawn the daemon with stdin/stdout/stderr closed.
5. On Windows, set `CREATE_NO_WINDOW` for the daemon process.
6. Wait for the named pipe to become reachable.

The `started_by_gui` flag is still useful for status reporting, but full close actions now ask the daemon to stop regardless of whether the GUI spawned it.

## UI Thread And Workers

egui rendering must stay fast. The GUI uses worker tasks for operations that can block or wait on IPC.

Worker messages include:

- download list snapshots
- selected download details
- add/control action results
- native-host status results

Registry checks and native-host installation do not run inside egui render functions. This prevents the Extension and Doctor windows from freezing the app.

The download list is rendered as a responsive table inside a two-axis scroll area. Column widths fit the current panel width, keep minimum usable sizes, and can be adjusted manually by dragging header dividers. If the window becomes too narrow for the minimum widths, the table scrolls horizontally instead of merging cells. The details side panel is optional and opens from a file-name double-click.

## IPC

Windows uses the same logical IPC protocol as Linux and macOS. Only the transport endpoint changes.

```text
Linux/macOS: Unix domain socket
Windows:     named pipe \\.\pipe\fhast-<user>
Format:      newline-delimited JSON
```

The GUI uses these request types most often:

- `ListDownloads`
- `DownloadInfo`
- `DownloadSegments`
- `DownloadHeaders`
- `RecentEvents`
- `AddDownload`
- `PauseDownload`
- `ResumeDownload`
- `RemoveDownload`
- `RetryDownload`
- `StopDaemon`

All requests include the IPC version constant from `fhast-ipc`.

## Config

The GUI loads and saves `ConfigFile` through `fhast-core`.

Windows config path:

```text
%APPDATA%\fhast\config.json
```

Important GUI-facing keys:

- `max_retries`
- `max_connections_per_download`
- `max_connections_global`
- `max_connections_per_host`
- `output_dir`
- `sensitive_header_retention`
- `chrome_extension_id`

The default save directory for GUI-added downloads is resolved in this order:

1. `ConfigFile.output_dir`
2. `%USERPROFILE%\Downloads`
3. the GUI executable directory

## Native Host Installation

Chrome Native Messaging on Windows is registry-backed. A manifest file alone is not enough.

In the GUI, the user-facing action is **Register Chrome Integration (Native Host)**. Under the hood, that action runs the Native Host helper `fhast-native-host.exe`.

The installer flow is:

1. Save `chrome_extension_id` to `%APPDATA%\fhast\config.json`.
2. Launch sibling `fhast-native-host.exe --install-host` with `fhast_EXTENSION_ID` set.
3. The native host writes `%APPDATA%\fhast\native-host\fhast_native_host.json`.
4. The native host registers `HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host` to point at that manifest.

The manifest allows only the configured extension origin:

```text
chrome-extension://<extension-id>/
```

The GUI checks Chrome integration and Native Host status by reading the registry and verifying the manifest path exists.

## Auto-Grab Detection

The extension and native host forward browser-captured downloads to the daemon. The native host adds this internal marker header:

```text
x-fhast-origin: native-host
```

The GUI checks recent events and selected download headers to report whether browser auto-grab has been seen recently. This is a status indicator only; the daemon remains the source of truth.

## Tray And Close Behavior

The tray code stores menu item handles and uses tray callback handlers. This avoids relying on a hidden egui window to poll tray events.

Window close behavior:

```text
X button -> prompt every time
Prompt Minimize to Tray -> minimize, leave daemon running
Prompt Close fhast -> quit GUI, stop daemon
Tray Show -> restore and focus
Tray Exit -> quit GUI, stop daemon
In-app Exit -> same as Tray Exit
```

The window is minimized instead of fully hidden so the app remains easy to wake and debug.

## Windows Console Policy

`fhast-gui` uses `#![cfg_attr(windows, windows_subsystem = "windows")]`, which prevents a console from appearing when the GUI itself is launched.

Any process spawned from the GUI must also avoid visible consoles. Current Windows subprocess rules:

- daemon spawn: `CREATE_NO_WINDOW`
- native-host installer spawn: `CREATE_NO_WINDOW`
- registry status query: `CREATE_NO_WINDOW`
- native-host registry registration: `CREATE_NO_WINDOW`

Avoid adding new `Command::new(...)` calls on Windows without applying the same rule or using a native Windows API instead.

## Security And Privacy Notes

- The GUI must not duplicate downloader logic from `fhast-core`.
- The native host must validate message schema and extension origin through Chrome's manifest allowlist.
- Sensitive headers must not be displayed or logged casually.
- Browser headers are captured only through explicit extension behavior.
- Save paths must continue to rely on core filename and path validation.
