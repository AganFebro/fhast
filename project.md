# Rust CLI/TUI Download Manager With Chrome Link Grabber

## Project definition

Build an open-source, terminal-first download manager inspired by IDM, but designed around Rust, a CLI, a TUI, and a Chrome extension used only as a download link grabber.

The product should not be a browser clone and should not try to bypass DRM, anti-bot systems, or paid bandwidth restrictions. Its value should be reliable resumable downloads, segmented HTTP acceleration when servers allow it, transparent browser handoff, good terminal UX, and privacy-respecting open-source behavior.

Recommended one-line definition:

> A Rust CLI/TUI download manager with a daemonized segmented downloader and a Chrome extension link grabber, focused on reliable resumable downloads, transparent browser handoff, and privacy-respecting open-source behavior.

The most important architecture rule:

```text
Downloader core owns state.
Daemon runs downloads.
CLI sends commands.
TUI displays and controls state.
Chrome extension captures links.
Native host bridges browser to daemon.
```

## Recommended architecture

Do not build this as one CLI process that downloads files directly. Build it as a small system with a persistent daemon.

```text
Chrome Extension
   |
   v
Rust Native Messaging Host
   |
   v
Local daemon: rdm-daemon
   |
   v
Download engine + queue + database

CLI: rdm add / pause / resume / list
TUI: rdm tui
```

The daemon should own every download. This allows downloads to continue even when the terminal UI is closed. The CLI and TUI should be clients that talk to the daemon over local IPC.

Suggested Rust workspace layout:

```text
rdm/
  Cargo.toml
  crates/
    rdm-core/          # download engine, segment logic, metadata models
    rdm-daemon/        # long-running background process
    rdm-cli/           # command-line interface
    rdm-tui/           # terminal dashboard
    rdm-native-host/   # Chrome Native Messaging bridge
    rdm-ipc/           # shared IPC message types
  extension/
    manifest.json
    background.ts
    content.ts
    native.ts
```

## Component responsibilities

### rdm-daemon

This is the real product. It should manage:

```text
download queue
pause and resume state
segment workers
retry policy
speed limiting
per-domain connection limits
metadata persistence
checksum verification
browser-submitted jobs
logs and events
```

The daemon should expose a local IPC API. Preferred choices:

```text
Linux/macOS: Unix domain socket
Windows: named pipe
Fallback: localhost HTTP with a random auth token
```

A localhost HTTP API is easier to debug, but it has a larger security surface. If used, reject unauthenticated requests, bind to 127.0.0.1 only, and require a per-user random token.

### rdm-cli

The CLI is a control client. It should not run downloads itself.

Example commands:

```bash
rdm daemon start
rdm daemon stop
rdm add "https://example.com/file.iso"
rdm add "https://example.com/file.iso" --connections 8 --out ~/Downloads/file.iso
rdm list
rdm pause <id>
rdm resume <id>
rdm remove <id>
rdm retry <id>
rdm info <id>
rdm config get
rdm config set max_connections_per_host 8
rdm doctor
rdm tui
```

Useful CLI output modes:

```bash
rdm list
rdm list --json
rdm info <id> --json
```

JSON output makes the tool scriptable.

### rdm-tui

The TUI is the dashboard and control surface.

Recommended screens:

```text
Active downloads
Queued downloads
Completed downloads
Failed downloads
Selected download details
Segment map
Speed graph
Recent logs
Per-domain rules
Settings
```

Recommended TUI controls:

```text
j/k or arrow keys: move selection
space: pause/resume selected download
a: add URL manually
r: retry failed download
d: delete/remove selected download
enter: open detail view
/: search/filter
q: quit TUI without stopping daemon
```

Important UX rule: quitting the TUI must not stop active downloads.

### rdm-native-host

This binary should be a thin adapter between Chrome and the daemon.

It should:

```text
read Chrome Native Messaging JSON from stdin
validate the sender/extension origin when possible
validate message schema
forward the job to rdm-daemon
return success or failure to Chrome
```

It should not contain the download engine. Chrome Native Messaging hosts communicate with Chrome through stdin/stdout using JSON messages prefixed by a 32-bit message length. Chrome may start host processes depending on the messaging pattern, so the native host should be treated as a bridge, not a persistent downloader.

### Chrome extension

The Chrome extension should be only the link grabber.

MVP capture methods:

```text
right-click link -> Download with RDM
toolbar button -> Send current page URL to RDM
manual URL entry popup -> Send to RDM
```

Later capture methods:

```text
detect file-like requests
show candidate media/file URLs
support optional host permissions per site
optionally capture selected request headers
```

Do not design the MVP around hijacking every browser download. Chrome Manifest V3 limits blocking webRequest behavior for most normal extensions. Treat request observation as detection and suggestion, not universal interception.

## Core downloader engine

The HTTP downloader flow should be:

```text
1. Receive URL plus optional headers from CLI or extension
2. Probe server capability
3. Detect file size, filename, validators, and range support
4. Choose single-connection or segmented mode
5. Create persistent job metadata
6. Start segment workers
7. Write parts safely
8. Retry failed segments when appropriate
9. Merge or finalize file
10. Verify checksum if available
11. Mark complete
```

Segmented downloading depends on HTTP range requests. A server may support them, ignore them, or reject them. Your engine must test and fall back safely.

Recommended probe:

```text
HEAD /file
Check:
  Content-Length
  Accept-Ranges
  ETag
  Last-Modified
  Content-Disposition

Then test:
  GET /file
  Range: bytes=0-0

Expected for segmented mode:
  206 Partial Content
  Content-Range: bytes 0-0/TOTAL
```

Do not rely only on `Accept-Ranges`. Actually test a small range request before using segmented mode.

## Segment strategy

### MVP: fixed segmentation

Example:

```text
File size: 1 GiB
Connections: 8

Segment 0: 0 MiB - 127 MiB
Segment 1: 128 MiB - 255 MiB
Segment 2: 256 MiB - 383 MiB
...
Segment 7: 896 MiB - 1023 MiB
```

Start with separate part files:

```text
file.iso.rdm/
  metadata.json
  segment-0.part
  segment-1.part
  segment-2.part
```

This is easier to implement and debug. After all segments complete, merge them into the final file.

### Later: positional writes

Eventually optimize into one preallocated partial file:

```text
file.iso.part
```

Each worker writes directly to its byte range. This is more efficient, but it requires careful offset writes, crash recovery, and corruption checks.

### Later: dynamic segmentation

Dynamic segmentation improves performance on uneven connections.

Example:

```text
Segment A still has 400 MiB remaining.
Worker B finished early.
The daemon splits the remaining range of Segment A.
Worker B helps download half of that remaining range.
```

Do this after the fixed segment downloader is already stable.

## Resume design

Every download should have durable metadata.

Example metadata:

```json
{
  "id": "job_123",
  "url": "https://example.com/file.iso",
  "final_path": "/home/user/Downloads/file.iso",
  "temp_dir": "/home/user/.local/state/rdm/jobs/job_123",
  "total_size": 1073741824,
  "etag": "\"abc123\"",
  "last_modified": "Tue, 05 May 2026 10:00:00 GMT",
  "headers": {
    "User-Agent": "Mozilla/5.0 ...",
    "Referer": "https://example.com/download"
  },
  "segments": [
    {
      "index": 0,
      "start": 0,
      "end": 134217727,
      "downloaded": 50000000,
      "state": "paused"
    }
  ]
}
```

Resume flow:

```text
1. Load metadata
2. Revalidate ETag and Last-Modified if available
3. If the file appears unchanged, continue incomplete segments
4. If the file changed, restart or require user confirmation
5. Use Range requests for incomplete parts
6. Use If-Range when possible
7. Verify final file size and optional checksum
```

Never blindly resume if the remote file changed. That can silently create corrupted output.

## Retry and failure behavior

Handle these cases:

```text
network timeout
DNS failure
connection reset
server closes a segment
HTTP 401/403
HTTP 429 rate limit
HTTP 5xx temporary error
expired signed URL
disk full
permission denied
file path conflict
range request rejected
server returns 200 instead of 206 for a range request
```

Recommended failure states:

```text
queued
probing
downloading
paused
merging
verifying
completed
failed
needs_refresh
cancelled
```

Retry policy:

```text
retry transient failures with exponential backoff
retry only failed segments when possible
limit retry attempts
pause or mark needs_refresh for repeated 401/403
respect Retry-After for 429 if present
fall back to single connection if segmented requests fail repeatedly
```

## Chrome extension link grabber

Start with explicit capture. This is easier, less invasive, and more trustworthy.

### Minimal Manifest V3 idea

```json
{
  "manifest_version": 3,
  "name": "RDM Link Grabber",
  "version": "0.1.0",
  "permissions": [
    "contextMenus",
    "nativeMessaging",
    "activeTab"
  ],
  "optional_permissions": [
    "downloads",
    "webRequest",
    "cookies"
  ],
  "optional_host_permissions": [
    "http://*/*",
    "https://*/*"
  ],
  "background": {
    "service_worker": "background.js"
  },
  "action": {
    "default_popup": "popup.html"
  }
}
```

### Capture levels

#### Level 1: explicit link capture

```text
User right-clicks a link.
Extension gets linkUrl and pageUrl.
Extension sends the URL to the native host.
Native host forwards it to daemon.
Daemon starts or queues the download.
```

This should be the MVP.

#### Level 2: browser download handoff

The extension can observe or interact with Chrome downloads where allowed, but do not assume Chrome will give you every header, cookie, or token needed to reproduce the request in your Rust downloader.

#### Level 3: media candidate detection

Use optional permissions to detect candidates like:

```text
.mp4
.webm
.m4a
.mp3
.zip
.iso
.m3u8
.mpd
application/octet-stream
video/*
audio/*
Content-Disposition: attachment
```

The extension can show candidates to the user. Avoid silent capture by default.

## Extension to native host message schema

Use a stable schema from the beginning.

```json
{
  "type": "add_download",
  "version": 1,
  "url": "https://cdn.example.com/file.zip?token=abc",
  "page_url": "https://example.com/download",
  "filename_hint": "file.zip",
  "headers": {
    "User-Agent": "Mozilla/5.0 ...",
    "Referer": "https://example.com/download"
  },
  "sensitive_headers": {
    "Cookie": "...",
    "Authorization": "..."
  },
  "options": {
    "start_immediately": true,
    "connections": null,
    "category": "archives"
  }
}
```

Security defaults:

```text
Do not store cookies permanently by default.
Do not log cookies or authorization headers.
Redact sensitive headers from logs.
Default sensitive header retention: until download completes.
Offer encrypted storage later via OS keychain.
```

Recommended setting:

```toml
[privacy]
store_sensitive_headers = "until_complete" # never | until_complete | encrypted
```

## Suggested Rust crates

Core runtime and HTTP:

```toml
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["stream", "cookies", "rustls-tls"] }
bytes = "1"
futures-util = "0.3"
url = "2"
```

CLI and TUI:

```toml
clap = { version = "4", features = ["derive"] }
ratatui = "0.29"
crossterm = "0.28"
```

Serialization, errors, logs:

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

Database:

```toml
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros", "migrate"] }
```

Or:

```toml
rusqlite = { version = "0.32", features = ["bundled"] }
```

Checksums:

```toml
sha2 = "0.10"
md-5 = "0.10"
```

Use SQLite for persistence. Downloads have relational state: jobs, segments, events, headers, queue ordering, and domain rules. SQLite is easy to inspect and repair.

## Suggested database schema

```sql
CREATE TABLE downloads (
  id TEXT PRIMARY KEY,
  url TEXT NOT NULL,
  final_path TEXT NOT NULL,
  temp_dir TEXT NOT NULL,
  status TEXT NOT NULL,
  total_size INTEGER,
  downloaded_bytes INTEGER NOT NULL DEFAULT 0,
  etag TEXT,
  last_modified TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE segments (
  id TEXT PRIMARY KEY,
  download_id TEXT NOT NULL,
  idx INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  downloaded_bytes INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL,
  last_error TEXT,
  FOREIGN KEY(download_id) REFERENCES downloads(id)
);

CREATE TABLE download_headers (
  download_id TEXT NOT NULL,
  name TEXT NOT NULL,
  value TEXT NOT NULL,
  sensitive INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(download_id) REFERENCES downloads(id)
);

CREATE TABLE domain_rules (
  domain TEXT PRIMARY KEY,
  max_connections INTEGER,
  speed_limit_bytes_per_sec INTEGER,
  auto_capture INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE events (
  id TEXT PRIMARY KEY,
  download_id TEXT,
  level TEXT NOT NULL,
  message TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(download_id) REFERENCES downloads(id)
);
```

## MVP scope

Build in this order:

| Phase | Build | Goal |
|---|---|---|
| 1 | Single-connection HTTP download | Prove file writing, progress, cancellation |
| 2 | Pause/resume | Force proper metadata and partial file design |
| 3 | Daemon + CLI | Establish the real architecture |
| 4 | Segmented downloads | Add IDM-like acceleration behavior |
| 5 | Retry/backoff + checksum | Make downloads reliable |
| 6 | TUI dashboard | Make it pleasant to use |
| 7 | Chrome context-menu extension | Add explicit link grabbing |
| 8 | Native Messaging host | Clean browser-to-daemon bridge |
| 9 | Optional request/media detection | Add IDM-like candidate detection |
| 10 | HLS/DASH support | Advanced media feature |

Do not start with HLS/DASH, media extraction, or a beautiful TUI. Start with a boring downloader that never corrupts files.

## First milestone details

Milestone 1 should support:

```text
rdm add <url>
rdm list
rdm pause <id>
rdm resume <id>
rdm info <id>
single active download
single connection
persistent metadata
safe temporary file
atomic rename on completion
```

Completion rule:

```text
Write to file.tmp first.
Flush and close.
Verify size if Content-Length is known.
Rename file.tmp to final filename atomically if possible.
```

## Second milestone details

Milestone 2 should add:

```text
multiple jobs
queue ordering
max active downloads
download progress events
basic retry policy
SQLite persistence
TUI read-only dashboard
```

At this point, the app becomes useful even before segmented downloads.

## Third milestone details

Milestone 3 should add segmented downloading:

```text
range probe
fixed segment planner
N segment workers
part files
merge step
per-host max connections
fallback to single connection
```

Default connection behavior:

```text
global max connections: 16
per-download default: 8
per-host default: 8
minimum segment size: 8 MiB
fallback: single connection if range test fails
```

## Fourth milestone details

Milestone 4 should add browser integration:

```text
Chrome extension context menu
Native Messaging host installer
native host manifest generator
extension message schema
forward add_download to daemon
TUI notification for browser-submitted jobs
```

Do not request cookies permission in the first version. Add it later as optional per-site permission.

## Security and privacy principles

This project can be more trustworthy than closed-source download managers if privacy is designed in from the beginning.

Rules:

```text
No telemetry by default.
No hidden remote calls.
No permanent cookie storage by default.
No sensitive headers in logs.
No automatic capture on every site by default.
No DRM bypass.
No anti-bot bypass.
No proxy/VPN limit evasion features.
Explicit user action for browser capture in MVP.
Optional permissions only when needed.
```

Sensitive data handling:

```text
Cookie: sensitive
Authorization: sensitive
Proxy-Authorization: sensitive
Set-Cookie: sensitive
X-Api-Key: sensitive
```

Redacted log example:

```text
Authorization: <redacted>
Cookie: <redacted>
```

## Hard problems to expect

### Range behavior is inconsistent

Some servers:

```text
support Range correctly
ignore Range and return 200 OK
return 416 Range Not Satisfiable
allow one range but reject parallel ranges
limit connections per IP/account/session
expire signed URLs quickly
```

Your app must treat segmented downloading as optional optimization, not a guarantee.

### Signed URLs expire

A CDN URL captured by the extension may work for only a few minutes.

For repeated 401/403 failures, mark the job:

```text
needs_refresh
```

TUI message:

```text
Link expired. Re-open the source page and capture the download again.
```

### Browser headers are sensitive

Headers can contain cookies and auth tokens. Make capture visible and temporary.

### Manifest V3 affects interception

Manifest V3 makes broad blocking request interception harder for normal extensions. Design around explicit capture and optional detection, not universal hijacking.

## Features to avoid in early versions

Avoid:

```text
DRM bypass
anti-bot bypass
automatic credential scraping
proxy/VPN-based limit evasion
blanket read-all-websites permission
torrent support
cloud-drive integrations
complex video site extractors
```

Those features increase legal, security, and maintenance risk. They will distract from the core product.

## Future features

After the MVP is stable:

```text
dynamic segmentation
per-site rules
scheduler
bandwidth profiles
checksum database/import
HLS playlist download
DASH manifest download
ffmpeg merge support
system tray integration
browser download handoff
import/export queue
remote daemon mode with authentication
```

HLS/DASH support should avoid DRM. For non-DRM media, the app can parse manifests, select quality, download segments, and merge audio/video with ffmpeg.

## Testing plan

Test categories:

```text
single file download
large file download
range-supported server
range-unsupported server
server returns 200 to Range request
server returns 206 correctly
connection reset during segment
pause/resume after process restart
file changed before resume
signed URL expiration
checksum mismatch
disk full
invalid filename
path collision
```

Recommended local test server behavior:

```text
normal file endpoint
range-supporting endpoint
range-ignoring endpoint
slow endpoint
flaky endpoint
403-after-N-seconds endpoint
connection-reset endpoint
```

Build a small local HTTP test server so you can reproduce edge cases without relying on random websites.

## Final recommended stack

```text
Language: Rust
Runtime: Tokio
HTTP client: reqwest first, hyper later if needed
CLI: clap
TUI: ratatui + crossterm
Database: SQLite via sqlx or rusqlite
Logging: tracing
Extension: Chrome Manifest V3, TypeScript
Browser bridge: Chrome Native Messaging
Daemon IPC: Unix socket / named pipe / authenticated localhost fallback
```

## References

- Chrome Native Messaging documentation: https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging
- Chrome permissions documentation: https://developer.chrome.com/docs/extensions/develop/concepts/declare-permissions
- Chrome webRequest API documentation: https://developer.chrome.com/docs/extensions/reference/api/webRequest
- Chrome blocking webRequest migration guidance: https://developer.chrome.com/docs/extensions/develop/migrate/blocking-web-requests
- Chrome contextMenus API documentation: https://developer.chrome.com/docs/extensions/reference/api/contextMenus
- Chrome downloads API documentation: https://developer.chrome.com/docs/extensions/reference/api/downloads
- HTTP Semantics RFC 9110, range requests and partial content: https://www.rfc-editor.org/rfc/rfc9110.html
- Tokio documentation: https://tokio.rs/
- Reqwest crate documentation: https://docs.rs/reqwest/
- Clap crate documentation: https://docs.rs/clap/
- Ratatui crate documentation: https://docs.rs/ratatui/latest/ratatui/index.html
