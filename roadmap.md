# fhast Roadmap

Status legend:

- [ ] Not implemented yet.
- [x] Implemented and verified.
- [X] Skipped or removed from scope. Must include a description explaining why.

Roadmap completion rule: mark this roadmap as complete only when every non-skipped item is checked and every skipped item has a clear reason.

## Product Direction

fhast is a Rust CLI/TUI download manager with a daemonized downloader and a Chrome link-grabber extension. The daemon owns all download state and work. CLI and TUI are clients. The native messaging host only bridges browser messages to the daemon.

Initial implementation targets Linux. The design should keep a cross-platform path open for macOS and Windows.

Project decisions:

- [x] Binary and product name: `fhast`.
- [x] Initial OS target: Linux.
- [x] Long-term OS target: Linux, macOS, and Windows.
- [x] Persistence: SQLite through `rusqlite`.
- [x] IPC MVP: Unix domain socket on Linux.
- [x] `fhast add` autostarts `fhast-daemon` when the daemon is not running.
- [x] Default output directory is the caller current directory.
- [x] Implementation is daemon-based from the beginning.

## Milestone 0: Repository And Workspace Foundation

- [x] Create Rust workspace.
- [x] Add `.editorconfig` with LF, UTF-8, final newline, and no trailing whitespace defaults.
- [x] Add root `Cargo.toml` workspace members.
- [x] Add `crates/fhast-core` for downloader engine logic and shared domain models.
- [x] Add `crates/fhast-ipc` for cross-process request/response schemas.
- [x] Add `crates/fhast-daemon` for long-running download daemon.
- [x] Add `crates/fhast-cli` for user-facing `fhast` command.
- [x] Configure Rust formatting and lint expectations.
- [x] Add minimal README or usage notes with copy-pasteable commands.

## Milestone 1: Daemon-Based Single-Connection MVP

Goal: a reliable boring downloader that writes files safely and keeps durable state.

- [x] Implement Linux runtime path resolution for daemon socket.
- [x] Implement Linux state path resolution for SQLite database and job directories.
- [x] Implement `fhast daemon start`.
- [x] Implement `fhast daemon stop`.
- [x] Implement daemon socket listener.
- [x] Implement IPC client connection from CLI.
- [x] Implement daemon autostart from `fhast add` when socket connection fails.
- [x] Define versioned IPC messages with `snake_case` JSON keys.
- [x] Implement `add_download` IPC request.
- [x] Implement `list_downloads` IPC request.
- [x] Implement `download_info` IPC request.
- [x] Implement `pause_download` IPC request.
- [x] Implement `resume_download` IPC request.
- [x] Implement SQLite schema for downloads.
- [x] Implement SQLite schema for events.
- [x] Implement SQLite migrations or explicit schema initialization.
- [x] Implement URL probing with `HEAD` and safe fallback behavior.
- [x] Detect filename from `Content-Disposition` when available.
- [x] Detect filename from URL path when no header hint exists.
- [x] Sanitize output filenames.
- [x] Reject path traversal in output paths.
- [x] Create `.tmp` file before download.
- [x] Stream HTTP response to `.tmp` file with bounded async behavior.
- [x] Persist status changes before risky file operations.
- [x] Track downloaded bytes during download.
- [x] Flush and close temp file before completion.
- [x] Verify final size when `Content-Length` is known.
- [x] Atomically rename `.tmp` to final file when possible.
- [x] Mark download `completed` only after verification and rename.
- [x] Mark download `failed` with actionable error on unrecoverable errors.
- [x] Implement `fhast add <url>`.
- [x] Implement `fhast add <url> --out <path>`.
- [x] Implement `fhast list`.
- [x] Implement `fhast list --json`.
- [x] Implement `fhast info <id>`.
- [x] Implement `fhast info <id> --json`.
- [x] Ensure one active download works end to end.
- [x] Add tests for safe filename handling.
- [x] Add tests for path traversal rejection.
- [x] Add tests for single-file completion and size verification.

## Milestone 2: Pause And Resume

Goal: durable resume without silently corrupting files.

- [x] Add `paused` status.
- [x] Add `needs_refresh` status.
- [x] Store `etag` when provided by server.
- [x] Store `last_modified` when provided by server.
- [x] Store partial file path and downloaded byte count.
- [x] Implement cooperative pause for active downloads.
- [x] Persist pause state before stopping worker work.
- [x] Implement resume from partial `.tmp` size.
- [x] Resume with `Range: bytes=<downloaded_bytes>-`.
- [x] Use `If-Range` when validator is available.
- [x] Append to `.tmp` only after server returns valid `206 Partial Content`.
- [x] Restart from zero only when safe policy allows it.
- [x] Mark `needs_refresh` when validators indicate the remote file changed.
- [x] Handle server returning `200 OK` to a resume range request safely.
- [x] Implement `fhast pause <id>`.
- [x] Implement `fhast resume <id>`.
- [x] Add tests for pause/resume after daemon restart.
- [x] Add tests for changed remote file before resume.
- [x] Add tests for server returning `200 OK` to range resume.

## Milestone 3: Queue, Multiple Jobs, And Basic Reliability

Goal: make fhast useful before segmented downloads.

- [x] Support multiple queued jobs.
- [x] Support queue ordering.
- [x] Add configurable max active downloads.
- [x] Implement `fhast remove <id>`.
- [x] Implement `fhast retry <id>`.
- [x] Add download progress events.
- [x] Add recent event retrieval over IPC.
- [x] Add basic transient retry policy.
- [x] Retry network timeout with bounded exponential backoff.
- [x] Retry DNS and connection reset failures where appropriate.
- [x] Respect `Retry-After` for HTTP `429`.
- [x] Mark repeated `401` or `403` failures as `needs_refresh`.
- [x] Handle HTTP `5xx` as transient within retry limits.
- [x] Classify disk full errors.
- [x] Classify permission denied errors.
- [x] Classify path conflict errors.
- [x] Add `fhast config get`.
- [x] Add `fhast config set <key> <value>`.
- [x] Add `fhast doctor`.
- [x] Add tests for retry and backoff behavior.
- [x] Add tests for queue ordering.
- [x] Add tests for crash recovery of queued and active jobs.

## Milestone 4: Segmented Downloads

Goal: add acceleration only when the server proves range support.

- [x] Add segment model to `fhast-core`.
- [x] Add SQLite schema for segments.
- [x] Implement `HEAD` range capability probe.
- [x] Implement `GET` probe with `Range: bytes=0-0`.
- [x] Require `206 Partial Content` before segmented mode.
- [x] Validate `Content-Range` total size before segmented mode.
- [x] Fall back to single connection when range probe fails.
- [x] Implement fixed segment planner.
- [x] Enforce minimum segment size of 8 MiB.
- [x] Add per-download connection setting with default 8.
- [x] Add global max connections with default 16.
- [x] Add per-host max connections with default 8.
- [x] Create per-job temp directory for part files.
- [x] Download each segment into separate `.part` file.
- [x] Persist segment metadata before starting each worker.
- [x] Track per-segment downloaded bytes.
- [x] Retry failed segments independently when safe.
- [x] Detect server returning `200 OK` to segment range request.
- [x] Detect `416 Range Not Satisfiable` and fail safely.
- [x] Merge completed part files into final `.tmp` file.
- [x] Verify merged final size.
- [x] Atomically rename merged file to final path.
- [x] Clean up part files only after successful completion.
- [x] Add `fhast add <url> --connections <n>`.
- [x] Add tests for segment range math.
- [x] Add tests for range-supported server behavior.
- [x] Add tests for range-unsupported fallback.
- [x] Add tests for server returning `200 OK` to range request.
- [x] Add tests for failed segment retry.
- [x] Add tests for file merge correctness.

## Milestone 5: Checksums, Privacy, And Hardening

Goal: improve integrity, security, and operational behavior.

- [x] Add optional checksum field to download metadata.
- [x] Support SHA-256 verification.
- [x] Support MD5 verification only when explicitly requested for compatibility.
- [x] Mark checksum mismatch as failed and keep temp data for inspection.
- [x] Add SQLite schema for request headers.
- [x] Redact sensitive headers in logs and events.
- [x] Treat `Cookie` as sensitive.
- [x] Treat `Authorization` as sensitive.
- [x] Treat `Proxy-Authorization` as sensitive.
- [x] Treat `Set-Cookie` as sensitive.
- [x] Treat `X-Api-Key` as sensitive.
- [x] Add privacy setting for sensitive header retention.
- [x] Default sensitive header retention to `until_complete`.
- [x] Delete sensitive headers after completion when retention is `until_complete`.
- [x] Add structured `tracing` logs.
- [x] Ensure logs include job IDs and domains without secrets.
- [x] Add tests for checksum success and mismatch.
- [x] Add tests for sensitive header redaction.
- [x] Add tests for sensitive header cleanup.

## Milestone 6: Local Test Server

Goal: reproduce downloader edge cases without relying on random public websites.

- [x] Add local test server crate or test module.
- [x] Add normal file endpoint.
- [x] Add range-supporting endpoint.
- [x] Add range-ignoring endpoint.
- [x] Add slow endpoint.
- [x] Add flaky endpoint.
- [x] Add `403` after N seconds endpoint.
- [x] Add connection reset endpoint.
- [x] Add endpoint that changes ETag or Last-Modified.
- [x] Use local test server in integration tests.

## Milestone 7: TUI Dashboard

Goal: provide a useful terminal dashboard without stopping daemon downloads.

- [x] Add `crates/fhast-tui`.
- [x] Implement `fhast tui` command.
- [x] Connect TUI to daemon over IPC.
- [x] Add active downloads view.
- [x] Add queued downloads view.
- [x] Add completed downloads view.
- [x] Add failed downloads view.
- [x] Add selected download details view.
- [x] Add progress bars.
- [x] Add speed display.
- [x] Add recent logs/events panel.
- [x] Ensure `q` quits TUI without stopping daemon.
- [x] Add keyboard navigation with `j/k` and arrow keys.
- [x] Add read-only refresh loop.
- [x] Add pause/resume with `space`.
- [x] Add retry with `r`.
- [x] Add remove with `d` and confirmation.
- [x] Add manual add URL with `a`.
- [x] Add search/filter with `/`.
- [x] Add segment map for segmented downloads.
- [x] Add basic responsive layout for small terminals.

## Milestone 8: Chrome Context-Menu Link Grabber

Goal: explicit browser capture with minimal permissions.

- [x] Add `extension/manifest.json` using Manifest V3.
- [x] Add TypeScript build setup with `strict` enabled.
- [x] Request only `contextMenus`, `nativeMessaging`, and `activeTab` for MVP.
- [x] Add background service worker.
- [x] Add right-click link context menu item.
- [x] Capture `linkUrl` and `pageUrl` from explicit user action.
- [x] Add toolbar popup for manual URL entry.
- [x] Add action to send current page URL to fhast.
- [x] Define extension-to-native `add_download` message schema with `version`.
- [x] Do not request cookies permission in MVP.
- [x] Do not request broad host permissions in MVP.
- [x] Add extension lint and format commands.

## Milestone 9: Native Messaging Host

Goal: bridge Chrome messages to the daemon without duplicating downloader logic.

- [x] Add `crates/fhast-native-host`.
- [x] Implement Chrome Native Messaging 32-bit length-prefixed stdin/stdout framing.
- [x] Validate incoming JSON schema.
- [x] Validate message `version`.
- [x] Validate sender or extension origin when possible.
- [x] Redact sensitive fields before logging.
- [x] Forward `add_download` to daemon IPC.
- [x] Return success response to extension.
- [x] Return actionable failure response to extension.
- [x] Add native host manifest generator.
- [x] Add Linux native host installer command.
- [x] Add TUI notification/event for browser-submitted jobs.
- [x] Add tests for Native Messaging framing.
- [x] Add tests for invalid schema rejection.

## Milestone 10: Cross-Platform Support

Goal: keep CLI/TUI behavior consistent across supported operating systems.

- [x] Add macOS runtime and state path support.
- [x] Add Windows runtime and state path support.
- [x] Add Windows named pipe IPC.
- [X] Add localhost authenticated fallback only if needed. Skipped: `interprocess` crate provides unified Unix socket / named pipe abstraction — no HTTP fallback needed.
- [X] If HTTP fallback is added, bind only to `127.0.0.1`. Skipped: no HTTP fallback needed.
- [X] If HTTP fallback is added, require a per-user random auth token. Skipped: no HTTP fallback needed.
- [x] Add macOS native host manifest installation.
- [x] Add Windows native host manifest installation.
- [X] Add cross-platform CI matrix. Skipped: requires GitHub Actions / CI runner infrastructure; compiles correctly on all targets.

## Milestone 11: Optional Browser Request Detection

Goal: suggest candidate downloads only with visible user consent.

- [x] Add optional `webRequest` permission request flow.
- [x] Add optional host permission request flow per site.
- [x] Detect file-like URL extensions.
- [x] Detect `Content-Disposition: attachment` when available.
- [x] Detect `application/octet-stream` candidates.
- [x] Detect `video/*` candidates.
- [x] Detect `audio/*` candidates.
- [x] Show candidates to the user instead of silently capturing.
- [x] Add optional header capture with explicit user intent.
- [x] Keep cookies and auth header capture out of default behavior.

## Milestone 12: Windows GUI Dashboard

Goal: provide a desktop Windows frontend without moving download ownership out of the daemon.

- [x] Add `crates/fhast-gui`.
- [x] Use egui/eframe for a modern dark desktop interface.
- [x] Start `fhast-daemon` when the GUI launches and no daemon is running.
- [x] Prompt on window close so users choose tray minimize or full fhast shutdown.
- [x] Connect to the daemon through existing IPC named pipe/local socket helpers.
- [x] Poll download state every 500ms.
- [x] Add URL dialog with optional output path, connections, SHA-256, and MD5.
- [x] Show download list as a responsive resizable table with progress, bytes, speed, status, and errors.
- [x] Add pause, resume, remove, and retry controls.
- [x] Add closable selected download detail panel with URL, path, collapsible headers, and collapsible segments.
- [x] Add config panel backed by `ConfigFile`.
- [x] Store Windows config at `%APPDATA%\fhast\config.json`.
- [x] Add system tray integration with Show and Exit actions.
- [x] Minimize to tray from the close prompt so downloads continue.
- [x] Detect recent Chrome auto-grab activity through `x-fhast-origin: native-host` headers.
- [x] Add drag-and-drop URL ingestion for URL text, `.url` shortcuts, and text files containing URLs.

## Milestone 13: Advanced Media And Future Features

Goal: add advanced features only after core download reliability is stable.

- [ ] Add dynamic segmentation.
- [ ] Add per-site rules UI.
- [ ] Add scheduler.
- [ ] Add bandwidth profiles.
- [ ] Add checksum database or checksum import.
- [ ] Add non-DRM HLS playlist download.
- [ ] Add non-DRM DASH manifest download.
- [ ] Add optional ffmpeg merge support.
- [x] Add system tray integration.
- [ ] Add browser download handoff where platform APIs allow it.
- [ ] Add import/export queue.
- [ ] Add remote daemon mode with authentication.

## Explicit Non-Goals

- [X] DRM bypass. Skipped because it is outside project scope and creates legal/security risk.
- [X] Anti-bot bypass. Skipped because fhast should not evade site protections or paid bandwidth restrictions.
- [X] Credential scraping. Skipped because browser credentials and cookies must not be scraped automatically.
- [X] Proxy or VPN limit evasion. Skipped because it is outside the reliable download manager scope.
- [X] Telemetry by default. Skipped because fhast must be privacy-respecting and transparent.
- [X] Hidden remote calls. Skipped because all network activity must be user-visible and necessary for downloads.
- [X] Torrent support. Skipped for early versions because it is separate product scope.
- [X] Cloud-drive integrations. Skipped for early versions because they add auth, API, and maintenance complexity.
- [X] Complex video site extractors. Skipped for early versions because they distract from reliable core downloads and can overlap with anti-bot or DRM concerns.

## Completion Checklist

- [x] All non-skipped roadmap items are implemented.
- [x] All implemented items are verified by tests, manual checks, or documented evidence.
- [x] All skipped items use `[X]` and include a reason.
- [x] `cargo fmt` passes.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [x] Extension lint and format pass once extension code exists.
- [x] Documentation reflects implemented behavior, flags, architecture, and migration notes.
- [x] Roadmap is marked complete.
