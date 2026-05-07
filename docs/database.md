# Database Schema

fhast uses SQLite via `rusqlite` at `~/.local/state/fhast/fhast.db`.

## Tables

### downloads

```sql
CREATE TABLE downloads (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    final_path TEXT NOT NULL,
    temp_path TEXT NOT NULL,
    status TEXT NOT NULL,
    total_size INTEGER,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    etag TEXT,
    last_modified TEXT,
    connections INTEGER NOT NULL DEFAULT 8,
    checksum_sha256 TEXT,
    checksum_md5 TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

### segments

```sql
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
```

### download_headers

```sql
CREATE TABLE download_headers (
    download_id TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    sensitive INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(download_id) REFERENCES downloads(id)
);
```

### events

```sql
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    download_id TEXT,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(download_id) REFERENCES downloads(id)
);
```

## Status Values

| Value | Description |
|---|---|
| `queued` | Waiting to be picked up |
| `probing` | Running HEAD + Range probe |
| `downloading` | Actively downloading |
| `paused` | Paused by user |
| `merging` | Merging segment part files |
| `verifying` | Running checksum check |
| `completed` | Successfully downloaded |
| `failed` | Unrecoverable error |
| `needs_refresh` | Remote file changed or URL expired |
| `cancelled` | Removed by user |

## Sensitive Headers

Headers marked `sensitive = 1` in `download_headers` are redacted from logs. Sensitive header names:

- `Cookie`
- `Authorization`
- `Proxy-Authorization`
- `Set-Cookie`
- `X-Api-Key`

## Migrations

Schema is initialized at first daemon startup with `CREATE TABLE IF NOT EXISTS`. No migration framework is used yet — tables are created idempotently.
