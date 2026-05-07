use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fhast_core::paths;
use fhast_ipc::{
    connect_to_daemon, read_message, split_stream, write_message, DownloadDto, EventDto,
    IpcRequest, IpcResponse, IPC_VERSION,
};
use tokio::io::BufReader;

#[derive(Debug, Parser)]
#[command(name = "fhast")]
#[command(about = "Daemon-backed CLI download manager")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    Add {
        url: String,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        connections: Option<u16>,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        md5: Option<String>,
    },
    List {
        #[arg(long)]
        json: bool,
    },
    Info {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Remove {
        id: String,
    },
    Retry {
        id: String,
    },
    Events {
        #[arg(long, default_value = "20")]
        limit: u32,
    },
    Tui,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Doctor,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Get { key: Option<String> },
    Set { key: String, value: String },
}

#[derive(Debug, Subcommand)]
enum DaemonCommand {
    Start,
    Stop,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { command } => match command {
            DaemonCommand::Start => {
                if start_daemon().await? {
                    println!("fhast-daemon started");
                } else {
                    println!("fhast-daemon is already running");
                }
            }
            DaemonCommand::Stop => match send_request(IpcRequest::StopDaemon {
                version: IPC_VERSION,
            })
            .await
            {
                Ok(response) => print_ack(response)?,
                Err(error) => anyhow::bail!("failed to stop daemon: {error}"),
            },
        },
        Commands::Add {
            url,
            out,
            connections,
            sha256,
            md5,
        } => {
            let request = IpcRequest::AddDownload {
                version: IPC_VERSION,
                url,
                output_path: out,
                working_dir: std::env::current_dir().context("resolve current directory")?,
                connections,
                checksum_sha256: sha256,
                checksum_md5: md5,
                headers: None,
                sensitive_headers: None,
            };
            let response = match send_request(request.clone()).await {
                Ok(response) => response,
                Err(_) => {
                    start_daemon().await?;
                    send_request(request)
                        .await
                        .context("send add_download request")?
                }
            };

            match response {
                IpcResponse::DownloadAdded { download, .. } => {
                    println!("queued {} -> {}", download.id, download.final_path);
                }
                IpcResponse::Error { message, .. } => anyhow::bail!(message),
                other => anyhow::bail!("unexpected daemon response: {other:?}"),
            }
        }
        Commands::List { json } => {
            let response = send_request(IpcRequest::ListDownloads {
                version: IPC_VERSION,
            })
            .await
            .context("send list_downloads request")?;
            match response {
                IpcResponse::DownloadList { downloads, .. } => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&downloads)?);
                    } else {
                        print_downloads(&downloads);
                    }
                }
                IpcResponse::Error { message, .. } => anyhow::bail!(message),
                other => anyhow::bail!("unexpected daemon response: {other:?}"),
            }
        }
        Commands::Info { id, json } => {
            let response = send_request(IpcRequest::DownloadInfo {
                version: IPC_VERSION,
                id,
            })
            .await
            .context("send download_info request")?;
            match response {
                IpcResponse::DownloadInfo { download, .. } => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&download)?);
                    } else {
                        print_download_detail(&download);
                    }
                }
                IpcResponse::Error { message, .. } => anyhow::bail!(message),
                other => anyhow::bail!("unexpected daemon response: {other:?}"),
            }
        }
        Commands::Pause { id } => {
            let response = send_request(IpcRequest::PauseDownload {
                version: IPC_VERSION,
                id,
            })
            .await
            .context("send pause_download request")?;
            print_ack(response)?;
        }
        Commands::Resume { id } => {
            let response = send_request(IpcRequest::ResumeDownload {
                version: IPC_VERSION,
                id,
            })
            .await
            .context("send resume_download request")?;
            print_ack(response)?;
        }
        Commands::Remove { id } => {
            let response = send_request(IpcRequest::RemoveDownload {
                version: IPC_VERSION,
                id,
            })
            .await
            .context("send remove_download request")?;
            print_ack(response)?;
        }
        Commands::Retry { id } => {
            let response = send_request(IpcRequest::RetryDownload {
                version: IPC_VERSION,
                id,
            })
            .await
            .context("send retry_download request")?;
            print_ack(response)?;
        }
        Commands::Events { limit } => {
            let response = send_request(IpcRequest::RecentEvents {
                version: IPC_VERSION,
                limit,
            })
            .await
            .context("send recent_events request")?;
            match response {
                IpcResponse::EventList { events, .. } => print_events(&events),
                IpcResponse::Error { message, .. } => anyhow::bail!(message),
                other => anyhow::bail!("unexpected daemon response: {other:?}"),
            }
        }
        Commands::Tui => {
            let mut tui_path = daemon_binary_path()?;
            tui_path.set_file_name("fhast-tui");
            let tui_binary = if tui_path.exists() {
                tui_path
            } else {
                PathBuf::from("fhast-tui")
            };

            let status = Command::new(&tui_binary)
                .status()
                .with_context(|| format!("spawn {}", tui_binary.display()))?;
            if !status.success() {
                anyhow::bail!("tui exited with status: {status}");
            }
        }
        Commands::Config { command } => match command {
            ConfigCommand::Get { key } => {
                let cf = fhast_core::load_config_file()?;
                print_config_value(&cf, key.as_deref());
            }
            ConfigCommand::Set { key, value } => {
                let mut cf = fhast_core::load_config_file()?;
                set_config_value(&mut cf, &key, &value)?;
                fhast_core::save_config_file(&cf)?;
                println!("set {key} = {value}");
            }
        },
        Commands::Doctor => {
            run_doctor().await;
        }
    }

    Ok(())
}

fn print_config_value(cf: &fhast_core::ConfigFile, key: Option<&str>) {
    match key {
        None | Some("") => {
            let json = serde_json::to_string_pretty(cf).unwrap_or_default();
            println!("{json}");
        }
        Some("max_retries") => println!("{}", cf.max_retries),
        Some("max_connections_per_download") => println!("{}", cf.max_connections_per_download),
        Some("max_connections_global") => println!("{}", cf.max_connections_global),
        Some("max_connections_per_host") => println!("{}", cf.max_connections_per_host),
        Some("output_dir") => println!(
            "{}",
            cf.output_dir
                .as_ref()
                .map_or("not set".to_string(), |p| p.display().to_string())
        ),
        Some("sensitive_header_retention") => println!("{}", cf.sensitive_header_retention),
        Some("chrome_extension_id") => println!(
            "{}",
            if cf.chrome_extension_id.is_empty() {
                "not set"
            } else {
                &cf.chrome_extension_id
            }
        ),
        Some(unknown) => eprintln!("unknown config key: {unknown}"),
    }
}

fn set_config_value(cf: &mut fhast_core::ConfigFile, key: &str, value: &str) -> Result<()> {
    match key {
        "max_retries" => cf.max_retries = value.parse().context("max_retries must be u8")?,
        "max_connections_per_download" => {
            cf.max_connections_per_download = value
                .parse()
                .context("max_connections_per_download must be u16")?
        }
        "max_connections_global" => {
            cf.max_connections_global = value
                .parse()
                .context("max_connections_global must be u16")?
        }
        "max_connections_per_host" => {
            cf.max_connections_per_host = value
                .parse()
                .context("max_connections_per_host must be u16")?
        }
        "output_dir" => {
            cf.output_dir = if value.is_empty() {
                None
            } else {
                Some(PathBuf::from(value))
            }
        }
        "sensitive_header_retention" => {
            if !matches!(value, "never" | "until_complete" | "encrypted") {
                anyhow::bail!(
                    "sensitive_header_retention must be: never, until_complete, or encrypted"
                );
            }
            cf.sensitive_header_retention = value.to_string();
        }
        "chrome_extension_id" => {
            cf.chrome_extension_id = value.trim().to_ascii_lowercase();
        }
        unknown => anyhow::bail!("unknown config key: {unknown}"),
    }
    Ok(())
}

async fn run_doctor() {
    println!("fhast doctor\n");
    println!("Configuration:");

    match fhast_core::config_path() {
        Ok(path) => {
            println!("  config file: {}", path.display());
            if path.exists() {
                println!("    status: found");
            } else {
                println!("    status: not found (defaults used)");
            }
        }
        Err(e) => println!("  config file: error ({e})"),
    }

    println!("\nDaemon socket:");
    match fhast_core::paths::socket_path() {
        Ok(path) => {
            println!("  path: {}", path.display());
            if path.exists() {
                println!("    status: socket exists");
            } else {
                println!("    status: socket not found (daemon not running?)");
            }
        }
        Err(e) => println!("  error resolving path: {e}"),
    }

    println!("\nDaemon connection:");
    match fhast_core::paths::socket_path() {
        Ok(socket_path) => match connect_to_daemon(&socket_path).await {
            Ok(_) => println!("  result: connected successfully"),
            Err(e) => println!("  result: connection failed ({e})"),
        },
        Err(e) => println!("  result: cannot resolve socket path ({e})"),
    }

    println!("\nDatabase:");
    match std::env::var("XDG_STATE_HOME") {
        Ok(xdg) => {
            let db = PathBuf::from(xdg).join("fhast/fhast.db");
            println!("  path: {}", db.display());
            match std::fs::metadata(&db) {
                Ok(m) => {
                    println!("    exists: yes");
                    println!("    size: {} bytes", m.len());
                }
                Err(_) => println!("    exists: no"),
            }
        }
        Err(_) => {
            let home = std::env::var("HOME").unwrap_or_default();
            let db = PathBuf::from(home).join(".local/state/fhast/fhast.db");
            println!("  path: {db}", db = db.display());
            match std::fs::metadata(&db) {
                Ok(m) => {
                    println!("    exists: yes");
                    println!("    size: {} bytes", m.len());
                }
                Err(_) => println!("    exists: no"),
            }
        }
    }

    println!("\nDisk space:");
    if let Ok(cwd) = std::env::current_dir() {
        println!("  working directory: {}", cwd.display());
    }
    // Check available space in current directory
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(".") {
            let _dev = meta.dev();
            // Simple check: can we write?
            let test = PathBuf::from(".fhast-doctor-test");
            match std::fs::write(&test, b"fhast-doctor") {
                Ok(()) => {
                    println!("  write test: passed");
                    let _ = std::fs::remove_file(&test);
                }
                Err(e) => println!("  write test: failed ({e})"),
            }
        }
    }

    println!("\nNative host:");
    let manifest_path = native_host_manifest_path();
    println!("  manifest: {}", manifest_path.display());

    #[cfg(windows)]
    match windows_native_host_manifest_value() {
        Some(path) => println!("    registry: registered ({})", path.display()),
        None => println!("    registry: missing"),
    }

    match std::fs::metadata(&manifest_path) {
        Ok(_) => {
            #[cfg(windows)]
            let status = if windows_native_host_manifest_value().is_some() {
                "installed"
            } else {
                "manifest exists, registry missing"
            };
            #[cfg(not(windows))]
            let status = "installed";
            println!("    status: {status}");
            match std::fs::read_to_string(&manifest_path) {
                Ok(content) => {
                    if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(path) = manifest.get("path").and_then(|v| v.as_str()) {
                            println!("    binary: {path}");
                            if PathBuf::from(path).exists() {
                                println!("    binary exists: yes");
                            } else {
                                println!("    binary exists: NO (build with `cargo build -p fhast-native-host`)");
                            }
                        }
                    }
                }
                Err(e) => println!("    error reading: {e}"),
            }
        }
        Err(_) => println!("    status: not installed (run `fhast-native-host --install-host`)"),
    }
}

fn native_host_manifest_path() -> PathBuf {
    #[cfg(windows)]
    {
        fhast_core::config_path()
            .ok()
            .and_then(|path| path.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("native-host")
            .join("fhast_native_host.json")
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(
            "Library/Application Support/Google/Chrome/NativeMessagingHosts/fhast_native_host.json",
        )
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("google-chrome/NativeMessagingHosts/fhast_native_host.json")
        } else {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home)
                .join(".config/google-chrome/NativeMessagingHosts/fhast_native_host.json")
        }
    }
}

#[cfg(windows)]
fn windows_native_host_manifest_value() -> Option<PathBuf> {
    const REGISTRY_KEY: &str =
        r"HKCU\Software\Google\Chrome\NativeMessagingHosts\fhast_native_host";

    let output = Command::new("reg")
        .args(["query", REGISTRY_KEY, "/ve"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .lines()
        .find_map(|line| line.split_once("REG_SZ").map(|(_, value)| value.trim()))?;
    if value.is_empty() {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

async fn send_request(request: IpcRequest) -> Result<IpcResponse> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    let stream = connect_to_daemon(&socket_path).await?;
    let (reader, mut writer) = split_stream(stream);
    let mut reader = BufReader::new(reader);

    write_message(&mut writer, &request).await?;
    let response = read_message(&mut reader).await?;

    Ok(response)
}

async fn start_daemon() -> Result<bool> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    if connect_to_daemon(&socket_path).await.is_ok() {
        return Ok(false);
    }

    let daemon = daemon_binary_path()?;
    Command::new(&daemon)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", daemon.display()))?;

    wait_for_daemon().await?;
    Ok(true)
}

async fn wait_for_daemon() -> Result<()> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;

    for _ in 0..50 {
        if connect_to_daemon(&socket_path).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    anyhow::bail!("daemon did not become ready at {}", socket_path.display())
}

fn daemon_binary_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("fhast_DAEMON") {
        return Ok(PathBuf::from(path));
    }

    let mut sibling = std::env::current_exe().context("resolve current executable")?;
    sibling.set_file_name("fhast-daemon");
    if sibling.exists() {
        return Ok(sibling);
    }

    Ok(PathBuf::from("fhast-daemon"))
}

fn print_ack(response: IpcResponse) -> Result<()> {
    match response {
        IpcResponse::Ok { message, .. } => {
            println!("{message}");
            Ok(())
        }
        IpcResponse::Error { message, .. } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected daemon response: {other:?}"),
    }
}

fn print_downloads(downloads: &[DownloadDto]) {
    println!("{:<36} {:<14} {:>12} path", "id", "status", "progress");
    for download in downloads {
        println!(
            "{:<36} {:<14} {:>12} {}",
            download.id,
            download.status,
            progress_text(download),
            download.final_path,
        );
    }
}

fn print_download_detail(download: &DownloadDto) {
    println!("id: {}", download.id);
    println!("url: {}", download.url);
    println!("path: {}", download.final_path);
    println!("status: {}", download.status);
    println!("progress: {}", progress_text(download));
    if let Some(speed) = download.downloaded_bytes_per_second {
        println!("speed: {}", format_speed(speed));
    }
    println!("connections: {}", download.requested_connections);
    println!("created_at: {}", download.created_at);
    println!("updated_at: {}", download.updated_at);
    if let Some(error) = &download.last_error {
        println!("last_error: {error}");
    }
}

fn print_events(events: &[EventDto]) {
    for event in events {
        let download = event.download_id.as_deref().unwrap_or("-");
        println!(
            "[{level}] {timestamp} {download_id} {message}",
            level = event.level.to_uppercase(),
            timestamp = event.created_at,
            download_id = download,
            message = event.message,
        );
    }
}

fn progress_text(download: &DownloadDto) -> String {
    match download.total_size {
        Some(total_size) if total_size > 0 => format!(
            "{:.1}%",
            (download.downloaded_bytes as f64 / total_size as f64) * 100.0
        ),
        Some(_) => "100.0%".to_owned(),
        None => format_bytes(download.downloaded_bytes),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = UNITS[0];

    for candidate in UNITS.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = candidate;
    }

    format!("{value:.1} {unit}")
}

fn format_speed(bytes_per_second: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_second as u64))
}
