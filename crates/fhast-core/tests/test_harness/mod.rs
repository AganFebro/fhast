use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use fhast_core::model::DownloadRecord;
use fhast_core::{CoreError, Storage};

pub const DATA_LEN: usize = 20 * 1024 * 1024;

#[derive(Clone)]
pub enum ServerMode {
    Range,
    IgnoreRange,
    FirstRangeOnlyThenIgnore,
    RangeWithSegmentFailure { fail_request_index: usize },
    RangeProbeOnlyThen503,
    Slow { read_delay_ms: u64 },
    ForbiddenAfterN { n: usize },
    ConnectionReset,
    ChangingEtag { requests_per_change: usize },
    ChangedData { change_after_request: usize },
    RangeResumeReturns200,
}

pub struct TestServer {
    address: String,
    pub requests: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
}

impl TestServer {
    pub fn start(mode: ServerMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        let requests_thread = requests.clone();
        let shutdown_thread = shutdown.clone();

        thread::spawn(move || {
            for stream in listener.incoming() {
                if shutdown_thread.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { break };
                handle_connection(stream, &mode, &requests_thread);
            }
        });

        Self {
            address: format!("http://{address}/file.bin"),
            requests,
            shutdown,
        }
    }

    pub fn url(&self) -> String {
        self.address.clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

fn handle_connection(mut stream: TcpStream, mode: &ServerMode, requests: &AtomicUsize) {
    let request_index = requests.fetch_add(1, Ordering::SeqCst) + 1;
    let mut buffer = [0; 4096];
    let bytes_read = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let is_head = request.starts_with("HEAD ");
    let range = parse_range(&request);

    if is_head {
        write_response(
            &mut stream,
            "200 OK",
            &[content_length_header(DATA_LEN)],
            None,
        );
        return;
    }

    match mode {
        ServerMode::ConnectionReset => {
            drop(stream);
            return;
        }
        ServerMode::ForbiddenAfterN { n } if request_index > *n && !is_head => {
            write_response(
                &mut stream,
                "403 Forbidden",
                &["Content-Length: 0".to_owned()],
                None,
            );
            return;
        }
        ServerMode::Slow { read_delay_ms } => {
            let data = test_data();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                data.len()
            );
            if stream.write_all(headers.as_bytes()).is_err() {
                return;
            }
            for chunk in data.chunks(64 * 1024) {
                if stream.write_all(chunk).is_err() {
                    return;
                }
                thread::sleep(Duration::from_millis(*read_delay_ms));
            }
            return;
        }
        _ => {}
    }

    match mode {
        ServerMode::IgnoreRange => {
            write_response(
                &mut stream,
                "200 OK",
                &[content_length_header(DATA_LEN)],
                Some(&test_data()),
            );
        }
        ServerMode::FirstRangeOnlyThenIgnore if request_index > 2 && range.is_some() => {
            write_response(
                &mut stream,
                "200 OK",
                &[content_length_header(DATA_LEN)],
                Some(&test_data()),
            );
        }
        ServerMode::ChangingEtag {
            requests_per_change,
        } => {
            let version = request_index / *requests_per_change;
            let data = test_data();
            let (start, end) = range.unwrap_or((0, data.len() - 1));
            let bounded_end = end.min(data.len() - 1);
            if start >= data.len() || start > bounded_end {
                write_response(
                    &mut stream,
                    "416 Range Not Satisfiable",
                    &["Content-Length: 0".to_owned()],
                    None,
                );
                return;
            }
            let body = &data[start..=bounded_end];
            write_response(
                &mut stream,
                "206 Partial Content",
                &[
                    format!("Content-Length: {}", body.len()),
                    format!("Content-Range: bytes {start}-{bounded_end}/{}", data.len()),
                    format!("ETag: \"v{version}\""),
                ],
                Some(body),
            );
        }
        ServerMode::RangeWithSegmentFailure { fail_request_index } => {
            let Some((start, end)) = range else {
                write_response(
                    &mut stream,
                    "200 OK",
                    &[content_length_header(DATA_LEN)],
                    Some(&test_data()),
                );
                return;
            };

            if request_index == *fail_request_index {
                write_response(
                    &mut stream,
                    "500 Internal Server Error",
                    &["Content-Length: 0".to_owned()],
                    None,
                );
                return;
            }

            let data = test_data();
            let bounded_end = end.min(data.len() - 1);
            if start >= data.len() || start > bounded_end {
                write_response(
                    &mut stream,
                    "416 Range Not Satisfiable",
                    &["Content-Length: 0".to_owned()],
                    None,
                );
                return;
            }
            let body = &data[start..=bounded_end];
            write_response(
                &mut stream,
                "206 Partial Content",
                &[
                    format!("Content-Length: {}", body.len()),
                    format!("Content-Range: bytes {start}-{bounded_end}/{}", data.len()),
                ],
                Some(body),
            );
        }
        ServerMode::RangeProbeOnlyThen503 => {
            let Some((start, end)) = range else {
                write_response(
                    &mut stream,
                    "200 OK",
                    &[content_length_header(DATA_LEN)],
                    Some(&test_data()),
                );
                return;
            };

            if start == 0 && end == 0 {
                let data = test_data();
                let body = &data[0..=0];
                write_response(
                    &mut stream,
                    "206 Partial Content",
                    &[
                        format!("Content-Length: {}", body.len()),
                        format!("Content-Range: bytes 0-0/{}", data.len()),
                    ],
                    Some(body),
                );
                return;
            }

            write_response(
                &mut stream,
                "503 Service Unavailable",
                &["Content-Length: 0".to_owned()],
                None,
            );
        }
        ServerMode::ChangedData {
            change_after_request,
        } => {
            let data = if request_index > *change_after_request {
                changed_test_data()
            } else {
                test_data()
            };
            let (start, end) = range.unwrap_or((0, data.len() - 1));
            let bounded_end = end.min(data.len() - 1);
            if start >= data.len() || start > bounded_end {
                write_response(
                    &mut stream,
                    "416 Range Not Satisfiable",
                    &["Content-Length: 0".to_owned()],
                    None,
                );
                return;
            }
            let body = &data[start..=bounded_end];
            write_response(
                &mut stream,
                "206 Partial Content",
                &[
                    format!("Content-Length: {}", body.len()),
                    format!("Content-Range: bytes {start}-{bounded_end}/{}", data.len()),
                    format!("ETag: \"v{request_index}\""),
                ],
                Some(body),
            );
        }
        ServerMode::RangeResumeReturns200 => {
            if range.is_some() {
                write_response(
                    &mut stream,
                    "200 OK",
                    &[content_length_header(DATA_LEN)],
                    Some(&test_data()),
                );
            } else {
                let data = test_data();
                write_response(
                    &mut stream,
                    "200 OK",
                    &[content_length_header(data.len())],
                    Some(&data),
                );
            }
        }
        ServerMode::Range | ServerMode::FirstRangeOnlyThenIgnore => {
            let Some((start, end)) = range else {
                write_response(
                    &mut stream,
                    "200 OK",
                    &[content_length_header(DATA_LEN)],
                    Some(&test_data()),
                );
                return;
            };

            let data = test_data();
            let bounded_end = end.min(data.len() - 1);
            if start >= data.len() || start > bounded_end {
                write_response(
                    &mut stream,
                    "416 Range Not Satisfiable",
                    &["Content-Length: 0".to_owned()],
                    None,
                );
                return;
            }
            let body = &data[start..=bounded_end];
            write_response(
                &mut stream,
                "206 Partial Content",
                &[
                    format!("Content-Length: {}", body.len()),
                    format!("Content-Range: bytes {start}-{bounded_end}/{}", data.len()),
                ],
                Some(body),
            );
        }
        _ => {}
    }
}

fn parse_range(request: &str) -> Option<(usize, usize)> {
    let range_line = request
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("range:"))?;
    let (_, value) = range_line.split_once('=')?;
    let (start, end) = value.trim().split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

fn write_response(stream: &mut TcpStream, status: &str, headers: &[String], body: Option<&[u8]>) {
    let mut response = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for header in headers {
        response.push_str(header);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    if stream.write_all(response.as_bytes()).is_err() {
        return;
    }
    if let Some(body) = body {
        let _ = stream.write_all(body);
    }
}

fn content_length_header(value: usize) -> String {
    format!("Content-Length: {value}")
}

pub fn test_data() -> Vec<u8> {
    (0..DATA_LEN).map(|index| (index % 251) as u8).collect()
}

fn changed_test_data() -> Vec<u8> {
    (0..DATA_LEN)
        .map(|index| ((index + 127) % 251) as u8)
        .collect()
}

pub fn segment_dir(temp_path: &Path) -> PathBuf {
    temp_path.with_extension("fhast-segments")
}

pub struct TestFixture {
    pub root: PathBuf,
    pub storage: Storage,
}

impl TestFixture {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("fhast-test-{}-{}", std::process::id(), unique_id()));
        std::fs::create_dir_all(&root).unwrap();
        let storage = Storage::open(root.join("fhast.db")).unwrap();
        Self { root, storage }
    }

    pub fn insert_download(
        &self,
        url: String,
        name: &str,
        connections: u16,
        checksum_sha256: Option<&str>,
    ) -> Result<DownloadRecord, CoreError> {
        let id = unique_id();
        let final_path = self.root.join(name);
        let temp_path = self.root.join(format!("{name}.tmp"));
        let mut record = DownloadRecord::new(id, url, final_path, temp_path);
        record.requested_connections = connections;
        record.checksum_sha256 = checksum_sha256.map(ToOwned::to_owned);
        self.storage.insert_download(&record)?;
        Ok(record)
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unique_id() -> String {
    use std::sync::atomic::AtomicUsize;
    static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
    NEXT_ID.fetch_add(1, Ordering::SeqCst).to_string()
}
