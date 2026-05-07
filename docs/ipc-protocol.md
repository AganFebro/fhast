# IPC Protocol

fhast processes communicate over a Unix domain socket using newline-delimited JSON. Every message includes a `version` field.

## Transport

- **Path**: `/run/user/<uid>/fhast/fhast.sock` (Linux)
- **Format**: JSON object terminated by `\n`
- **Encoding**: UTF-8
- **Flow**: Request → Response (synchronous request-reply per connection)

## Common Fields

Every message (request and response) includes:

```json
{ "type": "...", "version": 1 }
```

Current version: `1`.

## Requests

### AddDownload

Queue a new download.

```json
{
  "type": "add_download",
  "version": 1,
  "url": "https://example.com/file.zip",
  "output_path": null,
  "working_dir": "/home/user/downloads",
  "connections": null,
  "checksum_sha256": null,
  "checksum_md5": null,
  "headers": null,
  "sensitive_headers": null
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `url` | string | yes | Download URL |
| `output_path` | string\|null | no | Custom output path (default: filename from URL in working_dir) |
| `working_dir` | string | yes | Directory for output when no explicit path |
| `connections` | u16\|null | no | Override segment count (default: 8) |
| `checksum_sha256` | string\|null | no | Expected SHA-256 hex digest |
| `checksum_md5` | string\|null | no | Expected MD5 hex digest |
| `headers` | [[string,string]]\|null | no | Request headers to send (e.g., Referer, User-Agent). Response headers prefixed with `x-fhast-rh:` |
| `sensitive_headers` | [[string,string]]\|null | no | Sensitive headers (Cookie, Authorization). Stored encrypted, redacted in logs/TUI |

Response: `DownloadAdded` → DownloadDto

### ListDownloads

List all downloads.

```json
{ "type": "list_downloads", "version": 1 }
```

Response: `DownloadList` → `[DownloadDto]`

### DownloadInfo

Get details for a single download.

```json
{ "type": "download_info", "version": 1, "id": "<uuid>" }
```

Response: `DownloadInfo` → DownloadDto

### DownloadSegments

Get segments for a download.

```json
{ "type": "download_segments", "version": 1, "id": "<uuid>" }
```

Response: `SegmentList` → `[SegmentDto]`

### DownloadHeaders

Get stored headers for a download (request and response).

```json
{ "type": "download_headers", "version": 1, "id": "<uuid>" }
```

Response: `HeaderList` → `[[name, value, sensitive]]` where `sensitive` is a boolean. Response headers have `x-fhast-rh:` prefix.

### PauseDownload

```json
{ "type": "pause_download", "version": 1, "id": "<uuid>" }
```

Response: `Paused` → DownloadDto

### ResumeDownload

```json
{ "type": "resume_download", "version": 1, "id": "<uuid>" }
```

Response: `Resumed` → DownloadDto

### RemoveDownload

```json
{ "type": "remove_download", "version": 1, "id": "<uuid>" }
```

Response: `Removed` → DownloadDto

### RetryDownload

```json
{ "type": "retry_download", "version": 1, "id": "<uuid>" }
```

Response: `Retried` → DownloadDto

### RecentEvents

```json
{ "type": "recent_events", "version": 1, "limit": 20 }
```

Response: `EventList` → `[EventDto]`

## Response Types

### DownloadList

```json
{ "type": "download_list", "version": 1, "downloads": [...] }
```

### DownloadInfo

```json
{ "type": "download_info", "version": 1, "download": {...} }
```

### DownloadAdded

```json
{ "type": "download_added", "version": 1, "download": {...} }
```

### SegmentList

```json
{ "type": "segment_list", "version": 1, "segments": [...] }
```

### EventList

```json
{ "type": "event_list", "version": 1, "events": [...] }
```

### HeaderList

```json
{ "type": "header_list", "version": 1, "headers": [["Content-Type", "application/zip", false], ["Cookie", "...", true]] }
```

Each header is a triple: `[name, value, sensitive]`.

### Error

```json
{ "type": "error", "version": 1, "message": "something went wrong" }
```

## DTO Schemas

### DownloadDto

```json
{
  "id": "uuid",
  "url": "https://...",
  "final_path": "/home/user/downloads/file.zip",
  "status": "downloading",
  "total_size": 104857600,
  "downloaded_bytes": 52428800,
  "downloaded_bytes_per_second": 2097152.0,
  "created_at": "2026-05-06T12:00:00Z",
  "updated_at": "2026-05-06T12:00:00Z",
  "last_error": null,
  "requested_connections": 8,
  "checksum_sha256": null,
  "checksum_md5": null,
  "header_count": 14
}
```

### SegmentDto

```json
{
  "id": "segment-uuid",
  "download_id": "uuid",
  "index": 0,
  "start_byte": 0,
  "end_byte": 134217727,
  "downloaded_bytes": 67108864,
  "status": "downloading",
  "last_error": null
}
```

### EventDto

```json
{
  "id": "event-uuid",
  "download_id": "uuid",
  "level": "info",
  "message": "download completed",
  "created_at": "2026-05-06T12:00:00Z"
}
```
