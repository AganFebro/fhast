use thiserror::Error;
use url::Url;

use crate::model::{SegmentRecord, SegmentStatus};

pub const MIN_SEGMENT_SIZE: u64 = 8 * 1024 * 1024;
pub const DEFAULT_GLOBAL_MAX_CONNECTIONS: u16 = 16;
pub const DEFAULT_PER_HOST_MAX_CONNECTIONS: u16 = 8;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SegmentPlanError {
    #[error("total size must be greater than zero")]
    Empty,
    #[error("segment count must be greater than zero")]
    NoConnections,
}

pub fn plan_fixed_segments(
    download_id: &str,
    total_size: u64,
    requested_connections: u16,
) -> Result<Vec<SegmentRecord>, SegmentPlanError> {
    if total_size == 0 {
        return Err(SegmentPlanError::Empty);
    }
    if requested_connections == 0 {
        return Err(SegmentPlanError::NoConnections);
    }

    let max_segments_by_size = (total_size / MIN_SEGMENT_SIZE).max(1);
    let segment_count = u64::from(requested_connections).min(max_segments_by_size) as usize;
    let base_size = total_size / segment_count as u64;
    let remainder = total_size % segment_count as u64;
    let mut start = 0;
    let mut segments = Vec::with_capacity(segment_count);

    for index in 0..segment_count {
        let extra = u64::from((index as u64) < remainder);
        let size = base_size + extra;
        let end = start + size - 1;
        segments.push(SegmentRecord {
            id: format!("{download_id}:{index}"),
            download_id: download_id.to_owned(),
            index: index as u32,
            start_byte: start,
            end_byte: end,
            downloaded_bytes: 0,
            status: SegmentStatus::Queued,
            last_error: None,
        });
        start = end + 1;
    }

    Ok(segments)
}

pub fn effective_segment_connections(
    requested_connections: u16,
    url: &str,
    global_max: u16,
    per_host_max: u16,
) -> u16 {
    let requested = requested_connections.max(1);
    let global_limited = requested.min(global_max);

    if Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some()
    {
        global_limited.min(per_host_max)
    } else {
        global_limited
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_fixed_segments, MIN_SEGMENT_SIZE};

    #[test]
    fn plans_contiguous_fixed_segments() {
        let segments = plan_fixed_segments("job", 100, 4).unwrap();

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start_byte, 0);
        assert_eq!(segments[0].end_byte, 99);

        let segments = plan_fixed_segments("job", MIN_SEGMENT_SIZE * 4, 4).unwrap();
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].start_byte, 0);
        assert_eq!(segments[0].end_byte, MIN_SEGMENT_SIZE - 1);
        assert_eq!(segments[3].start_byte, MIN_SEGMENT_SIZE * 3);
        assert_eq!(segments[3].end_byte, MIN_SEGMENT_SIZE * 4 - 1);
    }

    #[test]
    fn caps_effective_connections_by_global_and_host_defaults() {
        assert_eq!(
            super::effective_segment_connections(64, "https://example.com/file", 16, 8),
            8
        );
        assert_eq!(
            super::effective_segment_connections(4, "https://example.com/file", 16, 8),
            4
        );
        assert_eq!(
            super::effective_segment_connections(0, "https://example.com/file", 16, 8),
            1
        );
        assert_eq!(
            super::effective_segment_connections(64, "not-a-url", 16, 8),
            16
        );
    }
}
