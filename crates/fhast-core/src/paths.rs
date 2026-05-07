use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::PermissionsExt;

pub fn runtime_dir() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let dir = match env::var_os("XDG_RUNTIME_DIR") {
            Some(value) => PathBuf::from(value).join("fhast"),
            None => {
                let uid = user_runtime_label();
                PathBuf::from("/run/user").join(&uid).join("fhast")
            }
        };
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
    #[cfg(target_os = "macos")]
    {
        let dir = match env::var_os("TMPDIR") {
            Some(v) => PathBuf::from(v),
            None => PathBuf::from("/tmp"),
        }
        .join(format!("fhast-{}", user_runtime_label()));
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
    #[cfg(windows)]
    {
        let dir = local_app_data_dir().join("fhast").join("runtime");
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
}

pub fn socket_path() -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    {
        let name = user_runtime_label();
        Ok(PathBuf::from(format!("\\\\.\\pipe\\fhast-{name}")))
    }
    #[cfg(not(windows))]
    {
        Ok(runtime_dir()?.join("fhast.sock"))
    }
}

pub fn state_dir() -> std::io::Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let dir = match env::var_os("XDG_STATE_HOME") {
            Some(value) => PathBuf::from(value).join("fhast"),
            None => home_dir()?.join(".local").join("state").join("fhast"),
        };
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
    #[cfg(target_os = "macos")]
    {
        let dir = home_dir()?
            .join("Library")
            .join("Application Support")
            .join("fhast");
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
    #[cfg(windows)]
    {
        let dir = local_app_data_dir().join("fhast");
        ensure_private_dir(&dir)?;
        Ok(dir)
    }
}

pub fn database_path() -> std::io::Result<PathBuf> {
    Ok(state_dir()?.join("fhast.db"))
}

pub fn jobs_dir() -> std::io::Result<PathBuf> {
    let dir = state_dir()?.join("jobs");
    ensure_private_dir(&dir)?;
    Ok(dir)
}

pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
    }

    #[cfg(windows)]
    {
        // No-op: Windows uses ACLs for directory permissions.
        // Default directory inheritance is sufficient for user-local state.
        let _ = path;
    }

    Ok(())
}

fn home_dir() -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    { env::var_os("HOME").map(PathBuf::from) }.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
    })
}

#[cfg(windows)]
fn local_app_data_dir() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            home_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("AppData")
                .join("Local")
        })
}

fn user_runtime_label() -> String {
    #[cfg(windows)]
    {
        env::var("USERNAME").unwrap_or_else(|_| std::process::id().to_string())
    }
    #[cfg(not(windows))]
    {
        env::var("UID")
            .or_else(|_| env::var("USER"))
            .unwrap_or_else(|_| std::process::id().to_string())
    }
}
