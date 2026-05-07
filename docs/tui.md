# TUI Dashboard

The TUI (`fhast tui`) is a read-only terminal dashboard built with `ratatui` + `crossterm`. It polls the daemon over IPC and renders download state.

## Launching

```bash
fhast tui
```

The TUI connects to the daemon's Unix socket and starts a 500ms refresh loop.

## Views

### Downloads View (default)

Shows all downloads grouped by status, with progress bars for active downloads.

```
 1  DOWNLOADING  file.zip  5.2 MiB / 100 MiB [████████░░░░░░░░░░░░] 52% [2.1 MB/s]
 2  QUEUED      video.mp4  0 B / 500 MiB
 3  COMPLETED   doc.pdf → /home/user/Downloads/doc.pdf
 4  FAILED      broken.bin  12 KiB (connection reset)
```

### Detail View (`Tab` to toggle)

Shows the selected download with:
- ID, URL, file name
- **Full output path** (where the file will be saved)
- Status, progress
- **Segment map** — colored bar showing segment states (green = complete, yellow = downloading, gray = pending)
- **Headers panel** — shows all stored headers for the download:
  - Cyan: request headers (Referer, User-Agent, etc.)
  - Red: sensitive headers (Cookie, Authorization) — values redacted
  - Yellow: response headers from the server (Content-Disposition, Content-Length, Content-Type) — prefixed `x-fhast-rh:` in storage, displayed without the prefix

### Events View (`Tab` to toggle into events mode)

Scrollable log of recent daemon events (download started, completed, failed, etc.).

## Keybindings

| Key | Action |
|---|---|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Space` | Pause/resume selected download |
| `r` | Retry failed download |
| `d` | Remove download (with confirmation) |
| `a` | Add URL manually (opens input bar) |
| `/` | Filter downloads (substring match on id/url) |
| `Enter` | Commit input / open detail |
| `Tab` | Toggle view: downloads → detail → events |
| `q` | Quit TUI (daemon keeps running) |

## Architecture

The TUI is a single `App` struct with:

```
App {
    ipc: IpcClient,           # Unix socket connection
    downloads: Vec<DownloadDto>,
    events: Vec<EventDto>,
    filter_text: String,
    input_mode: InputMode,    # Normal | AddUrl | Filter
    input_buffer: String,
    selected: usize,
    view_mode: ViewMode,      # Downloads | Detail | Events
}
```

The main loop runs `tokio::select!` between:
- Keyboard input (crossterm event stream)
- IPC poll timer (500ms → `list_downloads` + `recent_events`)
- Render tick (ratatui terminal draw)

The TUI never touches SQLite or the download engine — it's a pure IPC client.

## Adding a View

Views are rendered by methods on `App`:

- `render_downloads(f, area)` — list view with sections
- `render_detail(f, area)` — selected download detail
- `render_events(f, area)` — scrollable event log
- `render_status_bar(f, area)` — bottom key hints

To add a new view:
1. Add variant to `ViewMode`
2. Add render method
3. Add a case in the `render` match block
4. Update the `Tab` keybinding toggle
