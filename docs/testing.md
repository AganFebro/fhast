# Testing

fhast has two testing layers: Rust unit/integration tests and an embedded local test server.

## Running Tests

```bash
cargo test                    # All Rust tests
cargo test -p fhast-core      # Core engine tests only
cargo test --test downloader_integration  # Integration tests only
```

## Test Server

The integration tests use an embedded HTTP test server in `crates/fhast-core/tests/test_harness/mod.rs`. It binds to `127.0.0.1:0` and runs in a background thread.

### Server Modes

| Mode | Behavior |
|---|---|
| `Normal` | Plain HTTP 200 with deterministic data |
| `RangeSupported` | Full range support, `206 Partial Content` |
| `RangeIgnored` | Returns 200 even for Range requests |
| `Slow(delay_ms)` | Delays each 64KB chunk by N ms |
| `Flaky(fail_every_n)` | Fails every Nth request, succeeds otherwise |
| `ConnectionReset` | Closes connection midway |
| `ForbiddenAfterN(n)` | Returns 403 after N requests |
| `RateLimited { retry_after_secs }` | Returns 429 with Retry-After |
| `ChangingEtag { requests_per_change }` | Returns different ETag each N requests |
| `ChangedData` | Returns different file content per request |
| `RangeThenIgnore` | First request supports Range, subsequent ignore |

### Writing Tests

```rust
#[tokio::test]
async fn my_test() {
    let server = TestServer::start(ServerMode::Normal, Some(20 * 1024 * 1024)).await;
    let ctx = TestContext::new(server.url()).await;

    let record = ctx.download_and_verify("test-file.bin", None).await;

    assert_eq!(record.status, DownloadStatus::Completed);
    let content = tokio::fs::read(&record.final_path).await.unwrap();
    assert_eq!(content, server.test_data());
}
```

### Test Harness

`TestContext` wraps a `HttpDownloader` with an in-memory SQLite storage and temporary directories. Key methods:

- `download_and_verify(name, checksum)` — runs download, asserts success, returns record
- `download_no_verify(name)` — runs download, returns outcome without asserting success
- `pause_and_resume(ctx, name)` — pauses mid-download via CancellationToken, resumes with new server

## Current Test Coverage (26 tests)

### Core library unit tests
| File | Tests |
|---|---|
| `filename.rs` | Safe filename sanitization, path traversal rejection, temp path generation |
| `paths.rs` | Socket path resolution, state path resolution |

### Integration tests (`downloader_integration.rs`)
| Test | Covers |
|---|---|
| `single_file_completion_and_size_verification` | Single connection end-to-end |
| `segmented_download_succeeds` | Segmented mode with Range support |
| `fallback_to_single_connection_when_range_unsupported` | Fallback behavior |
| `server_returns_200_to_range_request` | Safe handling of 200 to Range |
| `failed_segment_retry_succeeds` | Per-segment retry |
| `file_merge_correctness` | Multi-part merge integrity |
| `pause_and_resume_single_connection` | Pause mid-stream, resume |
| `pause_and_resume_after_daemon_restart_simulation` | Resume with partial bytes |
| `detects_changed_remote_file_before_resume` | ETag change detection |
| `server_returns_200_to_range_resume` | 200-to-Range on resume |
| `retries_transient_failures_with_backoff` | Flaky server, retry delay |
| `connection_reset_handling` | Graceful connection reset |
| `repeated_401_marks_needs_refresh` | 403 → needs_refresh |
| `checksum_sha256_success_and_mismatch` | Checksum verification |
| `sensitive_header_redaction` | Log redaction |
| `sensitive_header_cleanup_after_completion` | Header retention policy |
| `queue_ordering_respected` | Insertion order listing |
| `crash_recovery_of_queued_jobs` | Persisted queue reload |
| `detects_changing_etag_during_download` | ETag change mid-download |

### Native host tests
| Test | Covers |
|---|---|
| `native_message_framing_roundtrip` | 4-byte length prefix framing |
| `invalid_json_rejection` | Schema validation |
| `missing_url_rejection` | Required field validation |
| `unsupported_version` | Version checking |
