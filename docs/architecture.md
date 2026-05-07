# Architecture

fhast is a multi-process system connected by IPC.

```text
Chrome Extension
   |
   v
fhast-native-host (stdin/stdout bridge)
   |
   v
fhast-daemon (owns state, runs downloads)
   |  |
   |  +-- fhast-cli (control client)
   |
   +-- fhast-tui (terminal dashboard)
  |
  +-- fhast-gui (Windows desktop dashboard)
```

## Crate Map

| Crate | Purpose | Key files |
|---|---|---|
| `fhast-core` | Download engine, storage, models, filename sanitization, paths | `downloader.rs`, `storage.rs`, `model.rs`, `filename.rs`, `paths.rs` |
| `fhast-ipc` | IPC message schemas and local socket transport | `message.rs`, `transport.rs`, `connection.rs` |
| `fhast-daemon` | Long-running process: socket listener, job scheduler, worker manager | `main.rs` |
| `fhast-cli` | User-facing CLI that sends IPC commands to daemon | `main.rs` |
| `fhast-tui` | Ratatui terminal dashboard, read-only IPC poll client | `main.rs` |
| `fhast-gui` | egui/eframe desktop dashboard for Windows | `main.rs`, `daemon.rs`, `ipc_client.rs`, `tray.rs` |
| `fhast-native-host` | Chrome Native Messaging bridge (stdin/stdout framing) | `main.rs` |
| `extension/` | Chrome Manifest V3 extension (TypeScript) | `background.ts`, `popup.ts`, `message_types.ts` |

## Data Flow

### Adding a download (CLI path)

```text
fhast add <url>
  |
  v
CLI connects to daemon socket (autostarts daemon if needed)
  |
  v
IpcRequest::AddDownload sent over Unix socket (newline-delimited JSON)
  |
  v
Daemon creates DownloadRecord, inserts into SQLite, responds IpcResponse::DownloadAdded
  |
  v
Daemon picks up queued job -> probes URL (HEAD + Range test) -> downloads
```

### Adding a download (Extension path)

```text
User right-clicks link -> "Send link to fhast"
  |
  v
Chrome sends JSON via 4-byte length-prefixed stdin to fhast-native-host
  |
  v
Native host validates message, forwards to daemon via Unix socket
  |
  v
Native host returns success/error to Chrome via 4-byte length-prefixed stdout
```

### Download life cycle

```text
queued -> probing -> downloading -> merging -> verifying -> completed
  |         |            |                              |
  |         +-- failed   +-- paused                     +-- failed
  +-- removed                                           +-- needs_refresh
```

## IPC Protocol

Transport: Unix domain socket (`/run/user/<uid>/fhast/fhast.sock`) on Linux/macOS, named pipe (`\\.\pipe\fhast-<user>`) on Windows.  
Format: newline-delimited JSON (`\n` terminated).  
Every message includes a `version` field (currently `1`).

### Request types

- `AddDownload` — queue a new download
- `ListDownloads` — list all downloads
- `DownloadInfo` — get single download details
- `DownloadSegments` — get segments for a download
- `PauseDownload` — pause an active download
- `ResumeDownload` — resume a paused download
- `RemoveDownload` — remove a download and clean files
- `RetryDownload` — retry a failed download
- `RecentEvents` — get recent daemon events

### Response types

- `DownloadList` — vec of DownloadDto
- `DownloadInfo` — single DownloadDto
- `DownloadAdded` — single DownloadDto
- `SegmentList` — vec of SegmentDto
- `EventList` — vec of EventDto
- `Paused` / `Resumed` / `Removed` / `Retried` — ack with DownloadDto
- `Error` — string message

## State & Persistence

- **Database**: SQLite via `rusqlite` at `~/.local/state/fhast/fhast.db` on Linux or `%LOCALAPPDATA%\fhast\fhast.db` on Windows
- **Job temp dirs**: `~/.local/state/fhast/jobs/<id>/`
- **Schema**: `downloads`, `segments`, `download_headers`, `events`

The daemon is the sole writer. CLI and TUI only read via IPC.

## Download Engine

The HTTP downloader in `fhast-core/src/downloader.rs` handles:

1. URL probing (`HEAD` + `Range: bytes=0-0` GET to test range support)
2. Single-connection or segmented mode (fallback to single if range test fails)
3. Fixed segment planner (min 8 MiB per segment, max 8 connections per download default)
4. Per-segment `.part` files in temp directory
5. Merge step after all segments complete
6. Atomic rename from `.tmp` to final path
7. Checksum verification (SHA-256, MD5 optional)
8. Resume: validates ETag/Last-Modified, uses `If-Range`, appends from `Range: bytes=N-`
9. Retry: failed segments retried independently, 200-to-range handled safely
