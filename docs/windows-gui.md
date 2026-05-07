# Windows GUI

`fhast-gui.exe` is the Windows desktop app for fhast. It gives Windows users a normal application window for adding downloads, watching progress, changing settings, and setting up Chrome integration.

For normal use, open `fhast-gui.exe`. The other `.exe` files are helpers that the GUI starts when it needs them.

Keep these files together in one folder:

```text
fhast-gui.exe
fhast-daemon.exe
fhast-native-host.exe
```

In plain terms:

- `fhast-gui.exe` is the app users open.
- `fhast-daemon.exe` is the background downloader used by the app.
- `fhast-native-host.exe` lets the Chrome extension send links to the app.

Users do not need to double-click the helper `.exe` files.

Technical note: the GUI does not download files directly. `fhast-daemon.exe` owns the download queue, database, HTTP requests, retries, resume state, and file writes. The GUI is a client that talks to the daemon over the same IPC protocol used by the CLI and TUI.

```text
fhast-gui.exe
  |
  | named pipe IPC: \\.\pipe\fhast-<user>
  v
fhast-daemon.exe
  |
  v
fhast-core download engine and SQLite state
```

## Quick Start

If you built from source with Cargo, create release `.exe` files first:

```powershell
cargo build --release
```

Then open:

```powershell
.\target\release\fhast-gui.exe
```

If you downloaded or received a release folder, skip the build command and double-click `fhast-gui.exe` in that folder.

On startup, the GUI starts the background downloader automatically if it is not already running. No terminal window should appear.

## Main Window

The main window shows the current download queue as a responsive table and shows daemon status in the toolbar/footer. Columns grow on wide windows, shrink down to useful minimums on smaller windows, and scroll horizontally only when there is no more safe space to compress. Drag the divider between column headers to resize columns manually.

Common actions:

- **Add URL** opens a dialog for a URL, optional save path, connection count, checksum, and custom headers.
- **Pause**, **Resume**, **Remove**, and **Retry** send control requests to the daemon for the selected download.
- Double-click a file name in the download table to open **Details**.
- **Details** shows the selected download URL, output path, status, and progress. Segments and captured headers are collapsible sections.
- **Events** shows recent daemon events, including queued, retry, failure, and completion messages.
- **Config** edits `%APPDATA%\fhast\config.json`.
- **Doctor** shows daemon, config, state, and native-host status.
- **Extension** stores the Chrome extension ID and registers Chrome integration through the Native Host helper (`fhast-native-host.exe`).

The GUI refreshes the download list on a short interval. Long-running work, including IPC calls and native-host status checks, runs outside the UI render path so the window stays responsive.

## Daemon Behavior

The daemon keeps downloads alive independently of the GUI window.

The in-app **Exit** button and tray **Exit** item close fhast and ask the daemon to stop.

Closing the window with **X** opens a choice prompt every time. **Minimize to Tray** keeps the daemon and downloads running. **Close fhast** closes the GUI and asks the daemon to stop.

## System Tray

The tray menu contains:

- **Show fhast** restores and focuses the GUI window.
- **Exit** quits fhast and asks the daemon to stop.

The tray controller uses native callbacks, so tray commands can wake the app even while the window is minimized.

## Chrome Extension Setup

The Chrome extension can send links and browser download captures to fhast. Chrome cannot talk to the daemon directly, so it launches `fhast-native-host.exe`. The native host validates Chrome messages and forwards them to the daemon over IPC.

Use the GUI setup flow:

1. Open `chrome://extensions/`.
2. Enable Developer mode and load the `extension/` folder.
3. Copy the extension ID shown by Chrome.
4. Open **Extension** in `fhast-gui.exe`.
5. Paste the extension ID and save it.
6. Click **Register Chrome Integration (Native Host)**.

That button registers the Chrome bridge backed by the Native Host helper, `fhast-native-host.exe`. If you click it again later, it refreshes the same registration instead of creating a second one.

On Windows, installing the native host writes:

```text
%APPDATA%\fhast\native-host\fhast_native_host.json
HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host
```

The registry key tells Chrome where the manifest file is. The manifest tells Chrome which executable to launch and which extension ID is allowed to use it.

## Auto-Grab Status

Auto-grab is active when the Chrome extension captures a browser download and the native host forwards it to the daemon. The native host marks forwarded downloads with the internal header `x-fhast-origin: native-host`. The GUI uses recent daemon events and captured headers to show whether browser integration has been seen recently.

## Drag And Drop

You can drag `http://` or `https://` text onto the GUI window. Dropped URLs are queued through the same AddDownload IPC path as the Add URL dialog.

## Windows Paths

| Purpose | Path |
|---|---|
| Config | `%APPDATA%\fhast\config.json` |
| Database | `%LOCALAPPDATA%\fhast\fhast.db` |
| Job temp files | `%LOCALAPPDATA%\fhast\jobs\<id>\` |
| Daemon IPC | `\\.\pipe\fhast-<user>` |
| Native host manifest | `%APPDATA%\fhast\native-host\fhast_native_host.json` |
| Native host registry | `HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host` |

## No Terminal Windows

The Windows GUI is built with the Windows subsystem, so launching `fhast-gui.exe` does not open a console. Processes started by the GUI, including the daemon and native-host installer, are started with `CREATE_NO_WINDOW`.

Small terminal flashes usually mean an older build is being used or a helper command was launched without that flag. The GUI hides the Windows registry helper commands it uses for native-host status and installation. CLI commands still run in a terminal by design.

## Troubleshooting

If the GUI says the daemon is unreachable, make sure `fhast-daemon.exe` is beside `fhast-gui.exe` or build it with:

```powershell
cargo build -p fhast-daemon
```

If Chrome says `Specified native messaging host not found`, open **Extension** in the GUI, verify the extension ID, and click **Register Chrome Integration (Native Host)** again.

If Chrome says access to the host is forbidden, the saved extension ID does not match the ID in `chrome://extensions/`. Paste the current ID into the GUI and reinstall the native host.
