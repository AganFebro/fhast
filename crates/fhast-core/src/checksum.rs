use std::fmt::Write as _;
use std::path::Path;

use md5::Md5;
use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Checksum {
    expected: String,
}

impl Sha256Checksum {
    pub fn new(expected: impl Into<String>) -> Self {
        Self {
            expected: normalize_hex(expected.into()),
        }
    }

    pub async fn verify_file(&self, path: &Path) -> Result<(), CoreError> {
        let actual = sha256_file(path).await?;
        if actual == self.expected {
            Ok(())
        } else {
            Err(CoreError::ChecksumMismatch {
                algorithm: "sha256",
                expected: self.expected.clone(),
                actual,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Md5Checksum {
    expected: String,
}

impl Md5Checksum {
    pub fn new(expected: impl Into<String>) -> Self {
        Self {
            expected: normalize_hex(expected.into()),
        }
    }

    pub async fn verify_file(&self, path: &Path) -> Result<(), CoreError> {
        let actual = md5_file(path).await?;
        if actual == self.expected {
            Ok(())
        } else {
            Err(CoreError::ChecksumMismatch {
                algorithm: "md5",
                expected: self.expected.clone(),
                actual,
            })
        }
    }
}

pub async fn sha256_file(path: &Path) -> Result<String, CoreError> {
    let mut file = File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex_string(&hasher.finalize()))
}

pub async fn md5_file(path: &Path) -> Result<String, CoreError> {
    let mut file = File::open(path).await?;
    let mut hasher = Md5::new();
    let mut buffer = vec![0; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex_string(&hasher.finalize()))
}

fn normalize_hex(value: String) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .or_else(|| value.trim().strip_prefix("md5:"))
        .unwrap_or_else(|| value.trim())
        .to_ascii_lowercase()
}

fn hex_string(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::Sha256Checksum;

    #[tokio::test]
    async fn verifies_sha256_file() {
        let path = std::env::temp_dir().join(format!("fhast-sha256-{}.txt", std::process::id()));
        tokio::fs::write(&path, b"fhast").await.unwrap();

        Sha256Checksum::new("8583c07addfc5f6f63a82e850dc830a6f87df86eaa7df37937b74c51f96ba260")
            .verify_file(&path)
            .await
            .unwrap();

        tokio::fs::remove_file(path).await.unwrap();
    }
}
