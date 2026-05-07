use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use fhast_core::paths;
use fhast_ipc::{connect_to_daemon, IpcRequest, IpcResponse, IPC_VERSION};

use crate::ipc_client;

pub struct DaemonManager {
    socket_path: PathBuf,
    started_by_gui: bool,
    child: Option<Child>,
}

impl DaemonManager {
    pub async fn ensure_running() -> Result<Self> {
        let socket_path = paths::socket_path().context("resolve daemon socket path")?;
        if connect_to_daemon(&socket_path).await.is_ok() {
            return Ok(Self {
                socket_path,
                started_by_gui: false,
                child: None,
            });
        }

        let daemon = daemon_binary_path()?;
        let mut command = Command::new(&daemon);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command
            .spawn()
            .with_context(|| format!("spawn {}", daemon.display()))?;

        wait_for_daemon(&socket_path).await?;

        Ok(Self {
            socket_path,
            started_by_gui: true,
            child: Some(child),
        })
    }

    pub fn offline(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            started_by_gui: false,
            child: None,
        }
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    pub fn started_by_gui(&self) -> bool {
        self.started_by_gui
    }

    pub async fn stop_if_started(&mut self) -> Result<Option<String>> {
        if !self.started_by_gui {
            return Ok(None);
        }

        let response = ipc_client::send_request(
            &self.socket_path,
            IpcRequest::StopDaemon {
                version: IPC_VERSION,
            },
        )
        .await;

        let message = match response {
            Ok(IpcResponse::Ok { message, .. }) => message,
            Ok(IpcResponse::Error { message, .. }) => message,
            Ok(other) => format!("unexpected daemon response: {other:?}"),
            Err(error) => format!("failed to ask daemon to stop: {error}"),
        };

        for _ in 0..30 {
            if connect_to_daemon(&self.socket_path).await.is_err() {
                self.started_by_gui = false;
                return Ok(Some(message));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if let Some(child) = &mut self.child {
            let _ = child.kill();
        }
        self.started_by_gui = false;
        Ok(Some(message))
    }
}

async fn wait_for_daemon(socket_path: &PathBuf) -> Result<()> {
    for _ in 0..50 {
        if connect_to_daemon(socket_path).await.is_ok() {
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

    let binary_name = if cfg!(windows) {
        "fhast-daemon.exe"
    } else {
        "fhast-daemon"
    };

    let mut sibling = std::env::current_exe().context("resolve current executable")?;
    sibling.set_file_name(binary_name);
    if sibling.exists() {
        return Ok(sibling);
    }

    Ok(PathBuf::from(binary_name))
}
