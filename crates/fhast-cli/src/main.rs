use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fhast_core::paths;
use fhast_ipc::{
    read_message, write_message, DownloadDto, EventDto, IpcRequest, IpcResponse, IPC_VERSION,
};
use tokio::io::BufReader;
use tokio::net::UnixStream;

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
    }

    Ok(())
}

async fn send_request(request: IpcRequest) -> Result<IpcResponse> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    let stream = UnixStream::connect(&socket_path)
        .await
        .with_context(|| format!("connect to daemon socket {}", socket_path.display()))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    write_message(&mut writer, &request).await?;
    let response = read_message(&mut reader).await?;

    Ok(response)
}

async fn start_daemon() -> Result<bool> {
    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    if UnixStream::connect(&socket_path).await.is_ok() {
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
        if UnixStream::connect(&socket_path).await.is_ok() {
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
