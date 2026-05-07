#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveHeaderRetention {
    Never,
    UntilComplete,
    Encrypted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyConfig {
    pub sensitive_header_retention: SensitiveHeaderRetention,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            sensitive_header_retention: SensitiveHeaderRetention::UntilComplete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FhastConfig {
    pub max_retries: u8,
    pub max_connections_per_download: u16,
    pub max_connections_global: u16,
    pub max_connections_per_host: u16,
    pub privacy: PrivacyConfig,
}

impl Default for FhastConfig {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_connections_per_download: 8,
            max_connections_global: 16,
            max_connections_per_host: 8,
            privacy: PrivacyConfig::default(),
        }
    }
}

use std::{env, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile {
    pub max_retries: u8,
    pub max_connections_per_download: u16,
    pub max_connections_global: u16,
    pub max_connections_per_host: u16,
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub sensitive_header_retention: String,
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_connections_per_download: 8,
            max_connections_global: 16,
            max_connections_per_host: 8,
            output_dir: None,
            sensitive_header_retention: "until_complete".to_string(),
        }
    }
}

impl From<ConfigFile> for FhastConfig {
    fn from(cf: ConfigFile) -> Self {
        let retention = match cf.sensitive_header_retention.as_str() {
            "never" => SensitiveHeaderRetention::Never,
            "encrypted" => SensitiveHeaderRetention::Encrypted,
            _ => SensitiveHeaderRetention::UntilComplete,
        };
        Self {
            max_retries: cf.max_retries,
            max_connections_per_download: cf.max_connections_per_download,
            max_connections_global: cf.max_connections_global,
            max_connections_per_host: cf.max_connections_per_host,
            privacy: PrivacyConfig {
                sensitive_header_retention: retention,
            },
        }
    }
}

impl From<FhastConfig> for ConfigFile {
    fn from(cfg: FhastConfig) -> Self {
        let retention = match cfg.privacy.sensitive_header_retention {
            SensitiveHeaderRetention::Never => "never",
            SensitiveHeaderRetention::UntilComplete => "until_complete",
            SensitiveHeaderRetention::Encrypted => "encrypted",
        };
        Self {
            max_retries: cfg.max_retries,
            max_connections_per_download: cfg.max_connections_per_download,
            max_connections_global: cfg.max_connections_global,
            max_connections_per_host: cfg.max_connections_per_host,
            output_dir: None,
            sensitive_header_retention: retention.to_string(),
        }
    }
}

fn config_dir() -> Result<PathBuf, std::io::Error> {
    #[cfg(windows)]
    {
        let base = env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData").join("Roaming"))
            })
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "APPDATA not set"))?;
        Ok(base.join("fhast"))
    }

    #[cfg(not(windows))]
    {
        let base = if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else {
            let home = env::var("HOME")
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
            PathBuf::from(home).join(".config")
        };
        Ok(base.join("fhast"))
    }
}

pub fn config_path() -> Result<PathBuf, std::io::Error> {
    Ok(config_dir()?.join("config.json"))
}

pub fn load_config_file() -> Result<ConfigFile, anyhow::Error> {
    let path = config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let cf: ConfigFile = serde_json::from_str(&content)?;
            Ok(cf)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_config_file(cf: &ConfigFile) -> Result<(), anyhow::Error> {
    let dir = config_dir()?;
    std::fs::create_dir_all(&dir)?;
    let path = config_path()?;
    let content = serde_json::to_string_pretty(cf)?;
    std::fs::write(&path, content)?;
    Ok(())
}
