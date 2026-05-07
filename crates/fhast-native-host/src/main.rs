use std::env;
use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::BufReader;

use fhast_core::paths;
use fhast_ipc::{
    connect_to_daemon, read_message, split_stream, write_message, IpcRequest, IpcResponse,
    IPC_VERSION,
};

const MANIFEST_EXTENSION_ID_HINT: &str = "fhast_EXTENSION_ID";

#[derive(Debug, Serialize)]
struct NativeManifest {
    name: &'static str,
    description: &'static str,
    path: String,
    #[serde(rename = "type")]
    host_type: &'static str,
    allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename = "add_download", rename_all = "snake_case")]
enum ExtensionMessage {
    #[serde(rename = "add_download")]
    AddDownload {
        version: u16,
        url: String,
        #[serde(default)]
        page_url: Option<String>,
        #[serde(default)]
        filename_hint: Option<String>,
        #[serde(default)]
        headers: serde_json::Value,
        #[serde(default)]
        sensitive_headers: serde_json::Value,
        #[serde(default)]
        response_headers: serde_json::Value,
        #[serde(default)]
        options: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ExtensionResponse {
    Success { version: u16, message: String },
    Error { version: u16, message: String },
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--manifest") => {
            print_manifest()?;
            return Ok(());
        }
        Some("--install-host") => {
            install_manifest()?;
            println!("native host manifest installed");
            return Ok(());
        }
        _ => {}
    }

    // Native messaging must ONLY write length-prefixed JSON to stdout.
    // Any logging output goes to stderr and is suppressed unless RUST_LOG is set.
    let log_filter = env::var("RUST_LOG").unwrap_or_else(|_| "off".into());
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(log_filter)
        .with_ansi(false)
        .init();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(run())
}

async fn run() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut reader = NativeMessageReader::new(stdin.lock());
    let mut writer = NativeMessageWriter::new(stdout.lock());

    loop {
        let raw = match reader.read_message() {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("fhast-native-host: failed to read native message: {e}");
                break;
            }
        };

        let request: ExtensionMessage = match serde_json::from_str(&raw) {
            Ok(req) => req,
            Err(e) => {
                eprintln!("fhast-native-host: invalid extension message: {e}");
                let response = ExtensionResponse::Error {
                    version: IPC_VERSION,
                    message: format!("invalid message: {e}"),
                };
                send_response(&raw, &response, &mut writer);
                continue;
            }
        };

        let response = match &request {
            ExtensionMessage::AddDownload {
                version,
                url,
                page_url,
                filename_hint,
                headers,
                sensitive_headers,
                response_headers,
                options,
            } => {
                handle_add_download(
                    *version,
                    url,
                    page_url.as_deref(),
                    filename_hint.as_deref(),
                    headers,
                    sensitive_headers,
                    response_headers,
                    options,
                )
                .await
            }
        };

        send_response(&raw, &response, &mut writer);
    }

    Ok(())
}

fn send_response(
    _raw_request: &str,
    response: &ExtensionResponse,
    writer: &mut NativeMessageWriter<io::StdoutLock>,
) {
    let serialized = serde_json::to_string(response).unwrap_or_else(|e| {
        serde_json::to_string(&ExtensionResponse::Error {
            version: IPC_VERSION,
            message: format!("serialize failure: {e}"),
        })
        .unwrap()
    });

    if let Err(e) = writer.write_message(&serialized) {
        eprintln!("fhast-native-host: failed to write response: {e}");
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_add_download(
    version: u16,
    url: &str,
    _page_url: Option<&str>,
    filename_hint: Option<&str>,
    headers: &serde_json::Value,
    sensitive_headers: &serde_json::Value,
    response_headers: &serde_json::Value,
    _options: &serde_json::Value,
) -> ExtensionResponse {
    if version != IPC_VERSION {
        return ExtensionResponse::Error {
            version: IPC_VERSION,
            message: format!("unsupported version {version}, expected {IPC_VERSION}"),
        };
    }

    if url.is_empty() {
        return ExtensionResponse::Error {
            version: IPC_VERSION,
            message: "url is required".into(),
        };
    }

    let output_path: Option<PathBuf> = filename_hint.map(PathBuf::from);
    let working_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut headers_vec = json_obj_to_kv_pairs(headers).unwrap_or_default();
    let rh_vec = json_obj_to_kv_pairs(response_headers).unwrap_or_default();
    for (k, v) in rh_vec {
        headers_vec.push((format!("x-fhast-rh:{k}"), v));
    }
    let sensitive_vec = json_obj_to_kv_pairs(sensitive_headers);

    let normal_count = headers_vec.len();
    let sensitive_count = sensitive_vec.as_ref().map_or(0, |v| v.len());
    eprintln!(
        "fhast-native-host: forwarding download with {} normal headers + {} sensitive headers",
        normal_count, sensitive_count
    );

    let request = IpcRequest::AddDownload {
        version: IPC_VERSION,
        url: url.to_string(),
        output_path,
        working_dir,
        connections: None,
        checksum_sha256: None,
        checksum_md5: None,
        headers: Some(headers_vec),
        sensitive_headers: sensitive_vec,
    };

    match forward_to_daemon(&request).await {
        Ok(response) => match response {
            IpcResponse::DownloadAdded { download, .. } => {
                eprintln!(
                    "fhast-native-host: download added id={} url={}",
                    download.id, download.url
                );
                ExtensionResponse::Success {
                    version: IPC_VERSION,
                    message: format!("queued {}", download.id),
                }
            }
            IpcResponse::Error { message, .. } => ExtensionResponse::Error {
                version: IPC_VERSION,
                message,
            },
            other => ExtensionResponse::Error {
                version: IPC_VERSION,
                message: format!("unexpected daemon response: {other:?}"),
            },
        },
        Err(e) => ExtensionResponse::Error {
            version: IPC_VERSION,
            message: format!("daemon connection failed: {e}"),
        },
    }
}

fn json_obj_to_kv_pairs(value: &serde_json::Value) -> Option<Vec<(String, String)>> {
    match value {
        serde_json::Value::Object(map) if !map.is_empty() => Some(
            map.iter()
                .map(|(k, v)| {
                    let val = if v.is_string() {
                        v.as_str().unwrap().to_string()
                    } else {
                        v.to_string()
                    };
                    (k.clone(), val)
                })
                .collect(),
        ),
        _ => None,
    }
}

async fn forward_to_daemon(request: &IpcRequest) -> Result<IpcResponse> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    let stream = connect_to_daemon(&socket_path).await?;
    let (reader, mut writer) = split_stream(stream);
    let mut reader = BufReader::new(reader);

    write_message(&mut writer, request).await?;
    let response = read_message(&mut reader).await?;

    Ok(response)
}

struct NativeMessageReader<R: Read> {
    inner: R,
}

impl<R: Read> NativeMessageReader<R> {
    fn new(inner: R) -> Self {
        Self { inner }
    }

    fn read_message(&mut self) -> Result<String> {
        let mut len_buf = [0u8; 4];
        self.inner
            .read_exact(&mut len_buf)
            .context("read message length")?;
        let len = u32::from_ne_bytes(len_buf);

        if len > 1024 * 1024 {
            anyhow::bail!("message too large: {len} bytes");
        }

        let mut payload = vec![0u8; len as usize];
        self.inner
            .read_exact(&mut payload)
            .context("read message payload")?;

        String::from_utf8(payload).context("invalid UTF-8 in message payload")
    }
}

struct NativeMessageWriter<W: Write> {
    inner: W,
}

impl<W: Write> NativeMessageWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner }
    }

    fn write_message(&mut self, message: &str) -> Result<()> {
        let payload = message.as_bytes();
        let len = payload.len() as u32;
        self.inner.write_all(&len.to_ne_bytes())?;
        self.inner.write_all(payload)?;
        self.inner.flush()?;
        Ok(())
    }
}

fn print_manifest() -> Result<()> {
    let extension_id =
        env::var("fhast_EXTENSION_ID").unwrap_or_else(|_| MANIFEST_EXTENSION_ID_HINT.to_string());

    let manifest = NativeManifest {
        name: "fhast_native_host",
        description: "fhast Native Messaging Host for Chrome link grabbing",
        path: env::current_exe()
            .context("resolve current executable")?
            .display()
            .to_string(),
        host_type: "stdio",
        allowed_origins: vec![format!("chrome-extension://{extension_id}/")],
    };
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

fn install_manifest() -> Result<()> {
    let extension_id = env::var("fhast_EXTENSION_ID")
        .ok()
        .or_else(discover_extension_id)
        .unwrap_or_else(|| MANIFEST_EXTENSION_ID_HINT.to_string());

    if extension_id == MANIFEST_EXTENSION_ID_HINT {
        anyhow::bail!(
            "Cannot determine Chrome extension ID.\n\
             Set fhast_EXTENSION_ID env var to your extension ID from chrome://extensions/"
        );
    }

    let manifest = NativeManifest {
        name: "fhast_native_host",
        description: "fhast Native Messaging Host for Chrome link grabbing",
        path: env::current_exe()
            .context("resolve current executable")?
            .display()
            .to_string(),
        host_type: "stdio",
        allowed_origins: vec![format!("chrome-extension://{extension_id}/")],
    };

    let native_dir = chrome_native_host_dir()?;
    std::fs::create_dir_all(&native_dir).context("create native messaging host directory")?;
    let manifest_path = native_dir.join("fhast_native_host.json");
    let payload = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, payload)
        .with_context(|| format!("write manifest to {}", manifest_path.display()))?;

    println!("manifest written to {}", manifest_path.display());
    Ok(())
}

fn discover_extension_id() -> Option<String> {
    // Chrome stores loaded extension IDs in its Preferences file.
    // Try reading from the Chrome user data directory.
    let chrome_dir = chrome_user_data_dir()?;
    let prefs = chrome_dir.join("Default").join("Preferences");
    let contents = std::fs::read_to_string(&prefs).ok()?;
    let prefs: serde_json::Value = serde_json::from_str(&contents).ok()?;

    // Look for fhast extension in the extensions.settings map
    let settings = prefs.get("extensions")?.get("settings")?;
    if let Some(obj) = settings.as_object() {
        for (id, ext) in obj {
            let name = ext.get("manifest")?.get("name")?.as_str().unwrap_or("");
            let path = ext.get("path")?.as_str().unwrap_or("");
            let manifest_path = ext.get("manifest")?.get("key")?.as_str().unwrap_or("");
            if name == "fhast Link Grabber"
                || name == "fhast"
                || path.contains("fhast")
                || manifest_path.contains("fhast")
            {
                return Some(id.clone());
            }
        }
    }

    None
}

fn chrome_user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME").ok()?;
        return Some(
            home.join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome"),
        );
    }
    #[cfg(windows)]
    {
        let local = env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                env::var("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("AppData")
                    .join("Local")
            });
        return Some(local.join("Google").join("Chrome").join("User Data"));
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(dir) = env::var("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(dir).join("google-chrome"));
        }
        let home = env::var("HOME").ok()?;
        Some(PathBuf::from(home).join(".config").join("google-chrome"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    None
}

fn chrome_native_host_dir() -> Result<PathBuf> {
    let user_data = chrome_user_data_dir().context("cannot find Chrome user data directory")?;
    Ok(user_data.join("NativeMessagingHosts"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_message_framing_roundtrip() {
        let test_msg =
            r#"{"type":"add_download","version":1,"url":"https://example.com/file.bin"}"#;

        let mut buf = Vec::new();
        {
            let mut writer = NativeMessageWriter::new(&mut buf);
            writer.write_message(test_msg).unwrap();
        }

        assert_eq!(buf.len(), 4 + test_msg.len());
        let len = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        assert_eq!(len as usize, test_msg.len());

        let mut reader = NativeMessageReader::new(buf.as_slice());
        let roundtripped = reader.read_message().unwrap();
        assert_eq!(roundtripped, test_msg);
    }

    #[test]
    fn test_invalid_json_rejection() {
        let raw = r#"{"type":"add_download","version":"not-a-number"}"#;
        let result: Result<ExtensionMessage> = serde_json::from_str(raw).map_err(Into::into);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_url_rejection() {
        let raw = r#"{"type":"add_download","version":1}"#;
        let result: Result<ExtensionMessage> = serde_json::from_str(raw).map_err(Into::into);
        assert!(result.is_err(), "missing url should fail deserialization");
    }

    #[test]
    fn test_unsupported_version() {
        let result = rt_block_on_handle_add(2, "https://example.com/file.bin", None, None);
        match result {
            ExtensionResponse::Error { message, .. } => {
                assert!(message.contains("unsupported version"));
            }
            _ => panic!("expected error response"),
        }
    }

    fn rt_block_on_handle_add(
        version: u16,
        url: &str,
        page_url: Option<&str>,
        hint: Option<&str>,
    ) -> ExtensionResponse {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(handle_add_download(
                version,
                url,
                page_url,
                hint,
                &serde_json::Value::Object(Default::default()),
                &serde_json::Value::Object(Default::default()),
                &serde_json::Value::Object(Default::default()),
                &serde_json::Value::Object(Default::default()),
            ))
    }
}
