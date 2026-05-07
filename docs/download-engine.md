# Download Engine

The download engine lives in `fhast-core` and is used exclusively by `fhast-daemon`.

## Architecture

```text
             URL + options
                 |
                 v
         HttpDownloader::download()
                 |
        +--------+--------+
        |                 |
   single conn      segmented mode
        |                 |
    stream to .tmp    N workers to .part files
        |                 |
        |            merge parts -> .tmp
        |                 |
        +--------+--------+
                 |
          verify size + optional checksum
                 |
          atomic rename .tmp -> final
```

## Modes

### Single Connection

Used when:
- Range probe fails (server returns 200 to `Range: bytes=0-0`)
- Server doesn't support `Accept-Ranges: bytes`
- Content-Length is unknown
- File is too small for segmentation (< 8 MiB)
- User specifies `--connections 1`

Flow:
1. Create `.tmp` file
2. Stream response body to `.tmp`
3. Track downloaded bytes
4. Flush and close
5. Verify final size (if Content-Length known)
6. Rename `.tmp` to final path

### Segmented Mode

Used when server proves range support via `206 Partial Content` response.

Flow:
1. Probe: `HEAD` to get Content-Length, ETag, Last-Modified
2. Probe: `GET` with `Range: bytes=0-0` to verify `206` response
3. Create temp directory `<job>/`
4. Plan segments (fixed-size, min 8 MiB each)
5. Create per-segment `.part` files
6. Spawn N workers (one per segment, bounded by connection limits)
7. Each worker: `GET` with `Range: bytes=start-end`, stream to `.part`
8. Track per-segment downloaded bytes
9. Retry failed segments independently
10. After all complete: merge `.part` files into `.tmp`
11. Verify merged size equals Content-Length
12. Rename `.tmp` to final path
13. Clean up `.part` files

## Segment Planner

```text
File size: 1 GiB = 1,073,741,824 bytes
Connections: 8
Min segment: 8 MiB = 8,388,608 bytes

Max segments by size: 1,073,741,824 / 8,388,608 = 128
Max segments by conns: min(8, 128) = 8

Segment count: 8
Segment 0: bytes 0 - 134,217,727
Segment 1: bytes 134,217,728 - 268,435,455
...
Segment 7: bytes 939,524,096 - 1,073,741,823
```

## Resume

Resume is handled at the download level (not per-segment for single connection).

### Single Connection Resume

1. Load download metadata from SQLite
2. If ETag/Last-Modified available: send `If-Range` with `Range: bytes=N-`
3. If server returns `206`: append to existing `.tmp`
4. If server returns `200`: restart from zero (only if policy allows)
5. If validators changed: mark `needs_refresh`

### Segmented Resume

1. Load all segment metadata
2. For each incomplete segment: resume from `downloaded_bytes` offset
3. For completed segments: skip
4. After all complete: merge and finalize

## Retry Policy

- **Transient network errors**: retry with exponential backoff (per segment)
- **HTTP 429**: respect `Retry-After` header
- **HTTP 401/403**: mark `needs_refresh` after repeated failures
- **HTTP 416**: fail segment (range not satisfiable)
- **200 to Range request**: restart segment from zero
- **DNS/connection reset**: retry within limits

## Connection Limits

| Limit | Default |
|---|---|
| Global max connections | 16 |
| Per-download default | 8 |
| Per-host max | 8 |
| Min segment size | 8 MiB |

## Checksum Verification

If a checksum is provided:
1. Download completes and file is renamed
2. SHA-256 or MD5 hash is computed on the final file
3. If mismatch: status set to `failed`, temp data kept for inspection
4. If match: status set to `completed`

## Request Headers

When headers are provided (via extension auto-grab or CLI `--header`), the downloader builds a `reqwest::Client` with those headers as `default_headers`. This means:

- Headers are sent with **every** request including redirect-following requests
- `Cookie` headers are forwarded as raw headers (no cookie jar needed since `cookies` feature is off)
- Response headers captured by the extension (Content-Disposition, Content-Length, Content-Type) are stored as `x-fhast-rh:HeaderName` prefixed headers in the `download_headers` table
- The Content-Disposition filename is used as the output filename (highest priority over Chrome's guess or URL parsing)
- Sensitive headers (Cookie, Authorization) are stored with `sensitive = 1` flag — redacted in TUI display and logs
- Headers are deleted after download completion when retention policy is `until_complete` (default)

## Safety Guarantees

- Never overwrite an existing final file without explicit intent
- Always write to `.tmp` first, rename atomically on success
- Persist state changes to SQLite before risky file operations
- Never leave ambiguous completion state
- Fall back to single connection when range behavior is uncertain
