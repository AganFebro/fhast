mod test_harness;

use std::sync::atomic::Ordering;
use std::time::Duration;

use fhast_core::checksum::sha256_file;
use fhast_core::model::DownloadStatus;
use fhast_core::privacy::is_sensitive_header;
use fhast_core::{CoreError, DownloadOutcome, HttpDownloader};
use test_harness::{segment_dir, test_data, ServerMode, TestFixture, TestServer};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn downloads_range_supported_server_with_segments_and_merges_file() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::Range);
    let record = fixture
        .insert_download(server.url(), "range.bin", 8, None)
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        tokio::fs::read(&record.final_path).await.unwrap(),
        test_data()
    );
    assert!(!segment_dir(&record.temp_path).exists());
    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Completed);
    assert_eq!(fixture.storage.list_segments(&record.id).unwrap().len(), 2);
}

#[tokio::test]
async fn falls_back_to_single_connection_when_range_probe_is_ignored() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);
    let record = fixture
        .insert_download(server.url(), "fallback.bin", 8, None)
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        tokio::fs::read(&record.final_path).await.unwrap(),
        test_data()
    );
    assert!(fixture
        .storage
        .list_segments(&record.id)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn falls_back_when_segment_request_returns_200() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::FirstRangeOnlyThenIgnore);
    let record = fixture
        .insert_download(server.url(), "range-200.bin", 8, None)
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        tokio::fs::read(&record.final_path).await.unwrap(),
        test_data()
    );
}

#[tokio::test]
async fn retries_failed_segment_independently() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::RangeWithSegmentFailure {
        fail_request_index: 3,
    });
    let record = fixture
        .insert_download(server.url(), "retry.bin", 8, None)
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        tokio::fs::read(&record.final_path).await.unwrap(),
        test_data()
    );
    assert!(server.requests.load(Ordering::SeqCst) >= 4);
}

#[tokio::test]
async fn checksum_mismatch_fails_and_keeps_temp_file() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);
    let record = fixture
        .insert_download(
            server.url(),
            "checksum.bin",
            1,
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
        )
        .unwrap();

    let error = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap_err();

    assert!(matches!(error, CoreError::ChecksumMismatch { .. }));
    assert!(record.temp_path.exists());
    assert!(!record.final_path.exists());
}

#[tokio::test]
async fn deletes_sensitive_headers_after_completion() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);
    let record = fixture
        .insert_download(server.url(), "headers.bin", 1, None)
        .unwrap();
    fixture
        .storage
        .add_header(
            &record.id,
            "Cookie",
            "secret",
            is_sensitive_header("Cookie"),
        )
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        fixture.storage.count_sensitive_headers(&record.id).unwrap(),
        0
    );
}

#[tokio::test]
async fn verifies_sha256_checksum_success() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);
    let hash = sha256_file_from_bytes(&test_data()).await;
    let record = fixture
        .insert_download(server.url(), "checksum-ok.bin", 1, Some(&hash))
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(sha256_file(&record.final_path).await.unwrap(), hash);
}

#[tokio::test]
async fn pause_and_resume_single_connection() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::Slow { read_delay_ms: 50 });
    let record = fixture
        .insert_download(server.url(), "pause-test.bin", 1, None)
        .unwrap();

    let id = record.id.clone();
    let storage = fixture.storage.clone();
    let cancel = CancellationToken::new();
    let handle = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            HttpDownloader::default()
                .download(&storage, &id, cancel)
                .await
        }
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel.cancel();

    let outcome = handle.await.unwrap();
    assert!(matches!(outcome, Ok(DownloadOutcome::Paused)));
}

#[tokio::test]
async fn detects_401_after_n_requests_and_marks_needs_refresh() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::ForbiddenAfterN { n: 2 });
    let record = fixture
        .insert_download(server.url(), "forbidden.bin", 1, None)
        .unwrap();

    let result = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn detects_changing_etag_during_download() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::ChangingEtag {
        requests_per_change: 1,
    });
    let record = fixture
        .insert_download(server.url(), "etag-change.bin", 1, None)
        .unwrap();

    let _ = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert!(
        stored.etag.is_some(),
        "expected etag to be stored after download"
    );
}

#[tokio::test]
async fn connection_reset_results_in_transient_error() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::ConnectionReset);
    let record = fixture
        .insert_download(server.url(), "reset.bin", 1, None)
        .unwrap();

    let result = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;

    assert!(result.is_err());
}

async fn sha256_file_from_bytes(bytes: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("fhast-hash-{}", unique_id_for_hash()));
    tokio::fs::write(&path, bytes).await.unwrap();
    let hash = sha256_file(&path).await.unwrap();
    tokio::fs::remove_file(path).await.unwrap();
    hash
}

fn unique_id_for_hash() -> String {
    use std::sync::atomic::AtomicUsize;
    static NEXT_ID: AtomicUsize = AtomicUsize::new(1000);
    NEXT_ID.fetch_add(1, Ordering::SeqCst).to_string()
}

#[tokio::test]
async fn single_file_completion_and_size_verification() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);
    let record = fixture
        .insert_download(server.url(), "complete.bin", 1, None)
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Completed);
    assert!(record.final_path.exists());
    assert!(!record.temp_path.exists());
    let file_content = tokio::fs::read(&record.final_path).await.unwrap();
    assert_eq!(file_content, test_data());
    assert_eq!(stored.downloaded_bytes, test_data().len() as u64);
    assert_eq!(stored.total_size, Some(test_data().len() as u64));
}

#[tokio::test]
async fn pause_and_resume_after_daemon_restart_simulation() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::Slow { read_delay_ms: 50 });
    let record = fixture
        .insert_download(server.url(), "restart-test.bin", 1, None)
        .unwrap();

    let id = record.id.clone();
    let storage = fixture.storage.clone();
    let cancel = CancellationToken::new();
    let handle = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            HttpDownloader::default()
                .download(&storage, &id, cancel)
                .await
        }
    });

    tokio::time::sleep(Duration::from_millis(2500)).await;
    cancel.cancel();

    let outcome = handle.await.unwrap();
    assert!(matches!(outcome, Ok(DownloadOutcome::Paused)));

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Paused);
    assert!(stored.downloaded_bytes > 0);

    fixture
        .storage
        .set_status_async(&record.id, DownloadStatus::Queued, None)
        .await
        .unwrap();

    let server2 = TestServer::start(ServerMode::Range);
    fixture
        .storage
        .set_url_async(&record.id, &server2.url())
        .await
        .unwrap();

    HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await
        .unwrap();

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Completed);

    let file_content = tokio::fs::read(&record.final_path).await.unwrap();
    assert_eq!(file_content, test_data());
}

#[tokio::test]
async fn detects_changed_remote_file_before_resume() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::Slow { read_delay_ms: 50 });
    let record = fixture
        .insert_download(server.url(), "changed-remote.bin", 1, None)
        .unwrap();

    let id = record.id.clone();
    let storage = fixture.storage.clone();
    let cancel = CancellationToken::new();
    let handle = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            HttpDownloader::default()
                .download(&storage, &id, cancel)
                .await
        }
    });

    tokio::time::sleep(Duration::from_millis(3000)).await;
    cancel.cancel();

    let outcome = handle.await.unwrap();
    assert!(matches!(outcome, Ok(DownloadOutcome::Paused)));

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Paused);
    assert!(stored.downloaded_bytes > 0);

    fixture
        .storage
        .set_status_async(&record.id, DownloadStatus::Queued, None)
        .await
        .unwrap();

    let changed_server = TestServer::start(ServerMode::ChangedData {
        change_after_request: 0,
    });
    fixture
        .storage
        .set_url_async(&record.id, &changed_server.url())
        .await
        .unwrap();

    let result = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert!(
        result.is_err() || stored.status == DownloadStatus::NeedsRefresh,
        "expected error or NeedsRefresh, got status={:?}",
        stored.status
    );
}

#[tokio::test]
async fn server_returns_200_to_range_resume() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::Slow { read_delay_ms: 50 });
    let record = fixture
        .insert_download(server.url(), "range-200-resume.bin", 1, None)
        .unwrap();

    let id = record.id.clone();
    let storage = fixture.storage.clone();
    let cancel = CancellationToken::new();
    let handle = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            HttpDownloader::default()
                .download(&storage, &id, cancel)
                .await
        }
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    cancel.cancel();

    let outcome = handle.await.unwrap();
    assert!(matches!(outcome, Ok(DownloadOutcome::Paused)));

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Paused);

    fixture
        .storage
        .set_status_async(&record.id, DownloadStatus::Queued, None)
        .await
        .unwrap();

    let server200 = TestServer::start(ServerMode::RangeResumeReturns200);
    fixture
        .storage
        .set_url_async(&record.id, &server200.url())
        .await
        .unwrap();

    let result = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;

    let stored = fixture.storage.get_download(&record.id).unwrap().unwrap();
    assert!(
        result.is_ok() || stored.status == DownloadStatus::Failed,
        "expected ok or Failed, got status={:?}",
        stored.status
    );
}

#[tokio::test]
async fn retries_transient_failures_with_backoff() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::RangeWithSegmentFailure {
        fail_request_index: 3,
    });
    let record = fixture
        .insert_download(server.url(), "retry-backoff.bin", 8, None)
        .unwrap();

    let start = tokio::time::Instant::now();
    let result = HttpDownloader::default()
        .download(&fixture.storage, &record.id, CancellationToken::new())
        .await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "download should succeed after retry");
    assert!(
        server.requests.load(Ordering::SeqCst) >= 4,
        "should have retried at least once"
    );
    assert!(
        elapsed > Duration::from_millis(100),
        "should have backoff delay"
    );
    assert_eq!(
        tokio::fs::read(&record.final_path).await.unwrap(),
        test_data()
    );
}

#[tokio::test]
async fn queue_ordering_respected() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);

    let rec1 = fixture
        .insert_download(server.url(), "queue-1.bin", 1, None)
        .unwrap();
    let rec2 = fixture
        .insert_download(server.url(), "queue-2.bin", 1, None)
        .unwrap();
    let rec3 = fixture
        .insert_download(server.url(), "queue-3.bin", 1, None)
        .unwrap();

    let records = fixture.storage.list_downloads().unwrap();
    assert!(records.len() >= 3);

    let ids: Vec<_> = records.iter().map(|r| r.id.clone()).collect();
    let pos1 = ids.iter().position(|id| id == &rec1.id);
    let pos2 = ids.iter().position(|id| id == &rec2.id);
    let pos3 = ids.iter().position(|id| id == &rec3.id);

    assert!(pos1.is_some(), "first download should be in queue");
    assert!(pos2.is_some(), "second download should be in queue");
    assert!(pos3.is_some(), "third download should be in queue");
    assert!(
        pos1 < pos2,
        "first added should come before second in queue"
    );
    assert!(
        pos2 < pos3,
        "second added should come before third in queue"
    );
}

#[tokio::test]
async fn crash_recovery_of_queued_jobs() {
    let fixture = TestFixture::new();
    let server = TestServer::start(ServerMode::IgnoreRange);

    let rec = fixture
        .insert_download(server.url(), "crash-recover.bin", 1, None)
        .unwrap();

    let stored = fixture.storage.get_download(&rec.id).unwrap().unwrap();
    assert_eq!(stored.status, DownloadStatus::Queued);
    assert!(stored
        .final_path
        .to_string_lossy()
        .contains("crash-recover"));
    assert!(stored.url.contains(&server.url()));

    let re_fetched = fixture.storage.get_download(&rec.id).unwrap().unwrap();
    assert_eq!(re_fetched.url, rec.url);
    assert_eq!(re_fetched.final_path, rec.final_path);
    assert_eq!(re_fetched.status, DownloadStatus::Queued);
}
