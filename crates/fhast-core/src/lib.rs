pub mod checksum;
pub mod config;
pub mod downloader;
pub mod error;
pub mod filename;
pub mod model;
pub mod paths;
pub mod privacy;
pub mod segment;
pub mod storage;

pub use checksum::{Md5Checksum, Sha256Checksum};
pub use config::{
    config_path, load_config_file, save_config_file, ConfigFile, FhastConfig, PrivacyConfig,
    SensitiveHeaderRetention,
};
pub use downloader::{DownloadOutcome, HttpDownloader};
pub use error::CoreError;
pub use filename::{resolve_output_path, sanitize_filename, OutputPaths};
pub use model::{DownloadRecord, DownloadStatus, EventRecord, SegmentRecord, SegmentStatus};
pub use segment::{plan_fixed_segments, SegmentPlanError, MIN_SEGMENT_SIZE};
pub use storage::Storage;
