use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn runtime_dir() -> std::io::Result<PathBuf> {
    let dir = match env::var_os("XDG_RUNTIME_DIR") {
        Some(value) => PathBuf::from(value).join("fhast"),
        None => env::temp_dir().join(format!("fhast-{}", user_runtime_label())),
    };

    ensure_private_dir(&dir)?;
    Ok(dir)
}

pub fn socket_path() -> std::io::Result<PathBuf> {
    Ok(runtime_dir()?.join("fhast.sock"))
}

pub fn state_dir() -> std::io::Result<PathBuf> {
    let dir = match env::var_os("XDG_STATE_HOME") {
        Some(value) => PathBuf::from(value).join("fhast"),
        None => home_dir()?.join(".local").join("state").join("fhast"),
    };

    ensure_private_dir(&dir)?;
    Ok(dir)
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

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions)?;
    }

    Ok(())
}

fn home_dir() -> std::io::Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME is not set"))
}

fn user_runtime_label() -> String {
    env::var("UID")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| std::process::id().to_string())
}
