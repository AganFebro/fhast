use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row};
use thiserror::Error;

use crate::model::{
    unix_timestamp_string, DownloadRecord, DownloadStatus, EventRecord, SegmentRecord,
    SegmentStatus,
};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("integer conversion failed for {0}")]
    IntegerConversion(&'static str),
    #[error("blocking storage task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
}

impl Storage {
    pub fn open(db_path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let storage = Self {
            db_path: db_path.into(),
        };

        if let Some(parent) = storage.db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = storage.connect()?;
        connection.execute_batch(SCHEMA)?;
        ensure_download_columns(&connection)?;
        drop(connection);

        Ok(storage)
    }

    pub fn insert_download(&self, download: &DownloadRecord) -> Result<(), StorageError> {
        self.connect()?.execute(
            "INSERT INTO downloads (
                id, url, final_path, temp_path, status, total_size, downloaded_bytes,
                etag, last_modified, checksum_sha256, checksum_md5, requested_connections,
                created_at, updated_at, last_error
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                download.id,
                download.url,
                download.final_path.to_string_lossy(),
                download.temp_path.to_string_lossy(),
                download.status.as_str(),
                option_u64_to_i64(download.total_size, "total_size")?,
                u64_to_i64(download.downloaded_bytes, "downloaded_bytes")?,
                download.etag,
                download.last_modified,
                download.checksum_sha256,
                download.checksum_md5,
                i64::from(download.requested_connections),
                download.created_at,
                download.updated_at,
                download.last_error,
            ],
        )?;

        Ok(())
    }

    pub async fn insert_download_async(
        &self,
        download: DownloadRecord,
    ) -> Result<(), StorageError> {
        self.with_blocking(move |storage| storage.insert_download(&download))
            .await
    }

    pub fn get_download(&self, id: &str) -> Result<Option<DownloadRecord>, StorageError> {
        self.connect()?
            .query_row(DOWNLOAD_SELECT_BY_ID, params![id], row_to_download)
            .optional()
            .map_err(StorageError::from)
    }

    pub async fn get_download_async(
        &self,
        id: impl Into<String>,
    ) -> Result<Option<DownloadRecord>, StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| storage.get_download(&id))
            .await
    }

    pub fn list_downloads(&self) -> Result<Vec<DownloadRecord>, StorageError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(DOWNLOAD_SELECT_ALL)?;
        let rows = statement.query_map([], row_to_download)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub async fn list_downloads_async(&self) -> Result<Vec<DownloadRecord>, StorageError> {
        self.with_blocking(|storage| storage.list_downloads()).await
    }

    pub fn set_url(&self, id: &str, url: &str) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads SET url = ?1, updated_at = ?2 WHERE id = ?3",
            params![url, unix_timestamp_string(), id],
        )?;
        Ok(())
    }

    pub async fn set_url_async(
        &self,
        id: impl Into<String>,
        url: &str,
    ) -> Result<(), StorageError> {
        let id = id.into();
        let url = url.to_owned();
        self.with_blocking(move |storage| storage.set_url(&id, &url))
            .await
    }

    pub fn first_queued_download(&self) -> Result<Option<DownloadRecord>, StorageError> {
        self.connect()?
            .query_row(
                DOWNLOAD_SELECT_FIRST_QUEUED,
                params![DownloadStatus::Queued.as_str()],
                row_to_download,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub async fn first_queued_download_async(
        &self,
    ) -> Result<Option<DownloadRecord>, StorageError> {
        self.with_blocking(|storage| storage.first_queued_download())
            .await
    }

    pub fn set_status(
        &self,
        id: &str,
        status: DownloadStatus,
        last_error: Option<&str>,
    ) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads SET status = ?1, updated_at = ?2, last_error = ?3 WHERE id = ?4",
            params![status.as_str(), unix_timestamp_string(), last_error, id],
        )?;

        Ok(())
    }

    pub async fn set_status_async(
        &self,
        id: impl Into<String>,
        status: DownloadStatus,
        last_error: Option<String>,
    ) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| storage.set_status(&id, status, last_error.as_deref()))
            .await
    }

    pub fn set_probe_info(
        &self,
        id: &str,
        total_size: Option<u64>,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads
             SET total_size = COALESCE(?1, total_size),
                 etag = COALESCE(?2, etag),
                 last_modified = COALESCE(?3, last_modified),
                 updated_at = ?4
             WHERE id = ?5",
            params![
                option_u64_to_i64(total_size, "total_size")?,
                etag,
                last_modified,
                unix_timestamp_string(),
                id,
            ],
        )?;

        Ok(())
    }

    pub async fn set_probe_info_async(
        &self,
        id: impl Into<String>,
        total_size: Option<u64>,
        etag: Option<String>,
        last_modified: Option<String>,
    ) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| {
            storage.set_probe_info(&id, total_size, etag.as_deref(), last_modified.as_deref())
        })
        .await
    }

    pub fn set_progress(&self, id: &str, downloaded_bytes: u64) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads SET downloaded_bytes = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                u64_to_i64(downloaded_bytes, "downloaded_bytes")?,
                unix_timestamp_string(),
                id,
            ],
        )?;

        Ok(())
    }

    pub async fn set_progress_async(
        &self,
        id: impl Into<String>,
        downloaded_bytes: u64,
    ) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| storage.set_progress(&id, downloaded_bytes))
            .await
    }

    pub fn complete_download(&self, id: &str, downloaded_bytes: u64) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads
             SET status = ?1, downloaded_bytes = ?2, updated_at = ?3, last_error = NULL
             WHERE id = ?4",
            params![
                DownloadStatus::Completed.as_str(),
                u64_to_i64(downloaded_bytes, "downloaded_bytes")?,
                unix_timestamp_string(),
                id,
            ],
        )?;

        Ok(())
    }

    pub async fn complete_download_async(
        &self,
        id: impl Into<String>,
        downloaded_bytes: u64,
    ) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| storage.complete_download(&id, downloaded_bytes))
            .await
    }

    pub fn replace_segments(
        &self,
        download_id: &str,
        segments: &[SegmentRecord],
    ) -> Result<(), StorageError> {
        let mut connection = self.connect()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM segments WHERE download_id = ?1",
            params![download_id],
        )?;

        for segment in segments {
            transaction.execute(
                "INSERT INTO segments (
                    id, download_id, idx, start_byte, end_byte, downloaded_bytes, status, last_error
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    segment.id,
                    segment.download_id,
                    i64::from(segment.index),
                    u64_to_i64(segment.start_byte, "start_byte")?,
                    u64_to_i64(segment.end_byte, "end_byte")?,
                    u64_to_i64(segment.downloaded_bytes, "downloaded_bytes")?,
                    segment.status.as_str(),
                    segment.last_error,
                ],
            )?;
        }

        transaction.commit()?;
        Ok(())
    }

    pub async fn replace_segments_async(
        &self,
        download_id: impl Into<String>,
        segments: Vec<SegmentRecord>,
    ) -> Result<(), StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.replace_segments(&download_id, &segments))
            .await
    }

    pub fn list_segments(&self, download_id: &str) -> Result<Vec<SegmentRecord>, StorageError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, download_id, idx, start_byte, end_byte, downloaded_bytes, status, last_error
             FROM segments WHERE download_id = ?1 ORDER BY idx ASC",
        )?;
        let rows = statement.query_map(params![download_id], row_to_segment)?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub async fn list_segments_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<Vec<SegmentRecord>, StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.list_segments(&download_id))
            .await
    }

    pub fn set_segment_status(
        &self,
        id: &str,
        status: SegmentStatus,
        downloaded_bytes: u64,
        last_error: Option<&str>,
    ) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE segments
             SET status = ?1, downloaded_bytes = ?2, last_error = ?3
             WHERE id = ?4",
            params![
                status.as_str(),
                u64_to_i64(downloaded_bytes, "downloaded_bytes")?,
                last_error,
                id,
            ],
        )?;

        Ok(())
    }

    pub async fn set_segment_status_async(
        &self,
        id: impl Into<String>,
        status: SegmentStatus,
        downloaded_bytes: u64,
        last_error: Option<String>,
    ) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| {
            storage.set_segment_status(&id, status, downloaded_bytes, last_error.as_deref())
        })
        .await
    }

    pub fn requeue_interrupted_downloads(&self) -> Result<(), StorageError> {
        self.connect()?.execute(
            "UPDATE downloads
             SET status = ?1, updated_at = ?2
             WHERE status IN (?3, ?4, ?5, ?6)",
            params![
                DownloadStatus::Queued.as_str(),
                unix_timestamp_string(),
                DownloadStatus::Probing.as_str(),
                DownloadStatus::Downloading.as_str(),
                DownloadStatus::Merging.as_str(),
                DownloadStatus::Verifying.as_str(),
            ],
        )?;
        self.connect()?.execute(
            "UPDATE segments SET status = ?1 WHERE status = ?2",
            params![
                SegmentStatus::Queued.as_str(),
                SegmentStatus::Downloading.as_str()
            ],
        )?;

        Ok(())
    }

    pub async fn requeue_interrupted_downloads_async(&self) -> Result<(), StorageError> {
        self.with_blocking(|storage| storage.requeue_interrupted_downloads())
            .await
    }

    pub fn add_header(
        &self,
        download_id: &str,
        name: &str,
        value: &str,
        sensitive: bool,
    ) -> Result<(), StorageError> {
        self.connect()?.execute(
            "INSERT INTO download_headers (download_id, name, value, sensitive)
             VALUES (?1, ?2, ?3, ?4)",
            params![download_id, name, value, i64::from(sensitive)],
        )?;

        Ok(())
    }

    pub fn delete_sensitive_headers(&self, download_id: &str) -> Result<(), StorageError> {
        self.connect()?.execute(
            "DELETE FROM download_headers WHERE download_id = ?1 AND sensitive = 1",
            params![download_id],
        )?;

        Ok(())
    }

    pub fn count_sensitive_headers(&self, download_id: &str) -> Result<u64, StorageError> {
        let count: i64 = self.connect()?.query_row(
            "SELECT COUNT(*) FROM download_headers WHERE download_id = ?1 AND sensitive = 1",
            params![download_id],
            |row| row.get(0),
        )?;

        u64::try_from(count).map_err(|_| StorageError::IntegerConversion("header_count"))
    }

    pub async fn delete_sensitive_headers_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<(), StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.delete_sensitive_headers(&download_id))
            .await
    }

    pub async fn add_headers_batch(
        &self,
        download_id: impl Into<String>,
        normal: impl Into<Vec<(String, String)>>,
        sensitive: impl Into<Vec<(String, String)>>,
    ) -> Result<(), StorageError> {
        let download_id = download_id.into();
        let normal = normal.into();
        let sensitive = sensitive.into();

        self.with_blocking(move |storage| {
            for (name, value) in &normal {
                storage.add_header(&download_id, name, value, false)?;
            }
            for (name, value) in &sensitive {
                storage.add_header(&download_id, name, value, true)?;
            }
            Ok(())
        })
        .await
    }

    pub fn get_headers(&self, download_id: &str) -> Result<Vec<(String, String)>, StorageError> {
        let connection = self.connect()?;
        let mut stmt = connection
            .prepare("SELECT name, value FROM download_headers WHERE download_id = ?1")?;
        let rows = stmt.query_map(params![download_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut headers = Vec::new();
        for row in rows {
            headers.push(row?);
        }
        Ok(headers)
    }

    pub fn get_headers_detail(
        &self,
        download_id: &str,
    ) -> Result<Vec<(String, String, bool)>, StorageError> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT name, value, sensitive FROM download_headers WHERE download_id = ?1",
        )?;
        let rows = stmt.query_map(params![download_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })?;

        let mut headers = Vec::new();
        for row in rows {
            headers.push(row?);
        }
        Ok(headers)
    }

    pub fn count_headers(&self, download_id: &str) -> Result<u32, StorageError> {
        let count: i64 = self.connect()?.query_row(
            "SELECT COUNT(*) FROM download_headers WHERE download_id = ?1",
            params![download_id],
            |row| row.get(0),
        )?;
        u32::try_from(count).map_err(|_| StorageError::IntegerConversion("header_count"))
    }

    pub async fn get_headers_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<Vec<(String, String)>, StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.get_headers(&download_id))
            .await
    }

    pub async fn get_headers_detail_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<Vec<(String, String, bool)>, StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.get_headers_detail(&download_id))
            .await
    }

    pub async fn count_headers_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<u32, StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.count_headers(&download_id))
            .await
    }

    pub fn delete_download(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connect()?;
        connection.execute("DELETE FROM segments WHERE download_id = ?1", params![id])?;
        connection.execute(
            "DELETE FROM download_headers WHERE download_id = ?1",
            params![id],
        )?;
        connection.execute("DELETE FROM events WHERE download_id = ?1", params![id])?;
        connection.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub async fn delete_download_async(&self, id: impl Into<String>) -> Result<(), StorageError> {
        let id = id.into();
        self.with_blocking(move |storage| storage.delete_download(&id))
            .await
    }

    pub fn delete_segments(&self, download_id: &str) -> Result<(), StorageError> {
        self.connect()?.execute(
            "DELETE FROM segments WHERE download_id = ?1",
            params![download_id],
        )?;
        Ok(())
    }

    pub async fn delete_segments_async(
        &self,
        download_id: impl Into<String>,
    ) -> Result<(), StorageError> {
        let download_id = download_id.into();
        self.with_blocking(move |storage| storage.delete_segments(&download_id))
            .await
    }

    pub fn add_event(
        &self,
        id: &str,
        download_id: Option<&str>,
        level: &str,
        message: &str,
    ) -> Result<(), StorageError> {
        self.connect()?.execute(
            "INSERT INTO events (id, download_id, level, message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, download_id, level, message, unix_timestamp_string()],
        )?;

        Ok(())
    }

    pub async fn add_event_async(
        &self,
        id: impl Into<String>,
        download_id: Option<String>,
        level: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), StorageError> {
        let id = id.into();
        let level = level.into();
        let message = message.into();
        self.with_blocking(move |storage| {
            storage.add_event(&id, download_id.as_deref(), &level, &message)
        })
        .await
    }

    pub fn recent_events(&self, limit: u32) -> Result<Vec<EventRecord>, StorageError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, download_id, level, message, created_at
             FROM events ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            Ok(EventRecord {
                id: row.get(0)?,
                download_id: row.get(1)?,
                level: row.get(2)?,
                message: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub async fn recent_events_async(&self, limit: u32) -> Result<Vec<EventRecord>, StorageError> {
        self.with_blocking(move |storage| storage.recent_events(limit))
            .await
    }

    fn connect(&self) -> Result<Connection, StorageError> {
        Ok(Connection::open(&self.db_path)?)
    }

    async fn with_blocking<T, F>(&self, function: F) -> Result<T, StorageError>
    where
        T: Send + 'static,
        F: FnOnce(Storage) -> Result<T, StorageError> + Send + 'static,
    {
        let storage = self.clone();
        tokio::task::spawn_blocking(move || function(storage)).await?
    }
}

const DOWNLOAD_SELECT_BY_ID: &str =
    "SELECT id, url, final_path, temp_path, status, total_size, downloaded_bytes,
    etag, last_modified, checksum_sha256, checksum_md5, requested_connections,
    created_at, updated_at, last_error FROM downloads WHERE id = ?1";
const DOWNLOAD_SELECT_ALL: &str =
    "SELECT id, url, final_path, temp_path, status, total_size, downloaded_bytes,
    etag, last_modified, checksum_sha256, checksum_md5, requested_connections,
    created_at, updated_at, last_error FROM downloads ORDER BY created_at ASC, id ASC";
const DOWNLOAD_SELECT_FIRST_QUEUED: &str = "SELECT id, url, final_path, temp_path, status, total_size, downloaded_bytes,
    etag, last_modified, checksum_sha256, checksum_md5, requested_connections,
    created_at, updated_at, last_error FROM downloads WHERE status = ?1 ORDER BY created_at ASC, id ASC LIMIT 1";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
  id TEXT PRIMARY KEY,
  url TEXT NOT NULL,
  final_path TEXT NOT NULL,
  temp_path TEXT NOT NULL,
  status TEXT NOT NULL,
  total_size INTEGER,
  downloaded_bytes INTEGER NOT NULL DEFAULT 0,
  etag TEXT,
  last_modified TEXT,
  checksum_sha256 TEXT,
  checksum_md5 TEXT,
  requested_connections INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_error TEXT
);

CREATE TABLE IF NOT EXISTS segments (
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

CREATE TABLE IF NOT EXISTS download_headers (
  download_id TEXT NOT NULL,
  name TEXT NOT NULL,
  value TEXT NOT NULL,
  sensitive INTEGER NOT NULL DEFAULT 0,
  FOREIGN KEY(download_id) REFERENCES downloads(id)
);

CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  download_id TEXT,
  level TEXT NOT NULL,
  message TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(download_id) REFERENCES downloads(id)
);
"#;

fn ensure_download_columns(connection: &Connection) -> Result<(), StorageError> {
    let mut statement = connection.prepare("PRAGMA table_info(downloads)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    let columns = rows.collect::<Result<HashSet<_>, _>>()?;

    if !columns.contains("checksum_sha256") {
        connection.execute("ALTER TABLE downloads ADD COLUMN checksum_sha256 TEXT", [])?;
    }
    if !columns.contains("checksum_md5") {
        connection.execute("ALTER TABLE downloads ADD COLUMN checksum_md5 TEXT", [])?;
    }
    if !columns.contains("requested_connections") {
        connection.execute(
            "ALTER TABLE downloads ADD COLUMN requested_connections INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }

    Ok(())
}

fn row_to_download(row: &Row<'_>) -> rusqlite::Result<DownloadRecord> {
    let status_text: String = row.get(4)?;
    let status = DownloadStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(error))
    })?;

    Ok(DownloadRecord {
        id: row.get(0)?,
        url: row.get(1)?,
        final_path: PathBuf::from(row.get::<_, String>(2)?),
        temp_path: PathBuf::from(row.get::<_, String>(3)?),
        status,
        total_size: optional_i64_to_u64(row.get(5)?, 5)?,
        downloaded_bytes: i64_to_u64(row.get(6)?, 6)?,
        etag: row.get(7)?,
        last_modified: row.get(8)?,
        checksum_sha256: row.get(9)?,
        checksum_md5: row.get(10)?,
        requested_connections: i64_to_u16(row.get(11)?, 11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        last_error: row.get(14)?,
    })
}

fn row_to_segment(row: &Row<'_>) -> rusqlite::Result<SegmentRecord> {
    let status_text: String = row.get(6)?;
    let status = SegmentStatus::from_str(&status_text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, Type::Text, Box::new(error))
    })?;

    Ok(SegmentRecord {
        id: row.get(0)?,
        download_id: row.get(1)?,
        index: i64_to_u32(row.get(2)?, 2)?,
        start_byte: i64_to_u64(row.get(3)?, 3)?,
        end_byte: i64_to_u64(row.get(4)?, 4)?,
        downloaded_bytes: i64_to_u64(row.get(5)?, 5)?,
        status,
        last_error: row.get(7)?,
    })
}

fn option_u64_to_i64(value: Option<u64>, field: &'static str) -> Result<Option<i64>, StorageError> {
    value.map(|inner| u64_to_i64(inner, field)).transpose()
}

fn u64_to_i64(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::IntegerConversion(field))
}

fn optional_i64_to_u64(value: Option<i64>, index: usize) -> rusqlite::Result<Option<u64>> {
    value.map(|inner| i64_to_u64(inner, index)).transpose()
}

fn i64_to_u64(value: i64, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn i64_to_u32(value: i64, index: usize) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn i64_to_u16(value: i64, index: usize) -> rusqlite::Result<u16> {
    u16::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}
