//! Port of AppPaths.cs + Linux Program.cs data-dir resolution.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub const LINUX_DATA_DIR: &str = "/var/lib/orginventory";
pub const MACOS_DATA_DIR: &str = "/var/db/orginventory";

/// Installer-created system data dir for the current OS.
/// Windows: `%ProgramData%\OrgInventory` (MSI service chạy LocalSystem).
pub fn system_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        return std::env::var("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"))
            .join("OrgInventory");
    }
    #[cfg(not(windows))]
    PathBuf::from(if cfg!(target_os = "macos") {
        MACOS_DATA_DIR
    } else {
        LINUX_DATA_DIR
    })
}

/// `--data-dir` > `ORGINV_DATA_DIR` > `ORGINVENTORY_DATA_DIR` > [`system_data_dir`] (if it exists
/// and is writable) > ~/.local/share/OrgInventory.
pub fn resolve_data_dir(cli: Option<&str>) -> PathBuf {
    if let Some(d) = cli.filter(|d| !d.trim().is_empty()) {
        return PathBuf::from(d);
    }
    for var in ["ORGINV_DATA_DIR", "ORGINVENTORY_DATA_DIR"] {
        if let Ok(v) = std::env::var(var) {
            if !v.trim().is_empty() {
                return PathBuf::from(v);
            }
        }
    }
    let system = system_data_dir();
    if system.is_dir() && is_writable(&system) {
        return system;
    }
    home_dir().join(".local").join("share").join("OrgInventory")
}

#[cfg(unix)]
fn is_writable(dir: &Path) -> bool {
    nix::unistd::access(dir, nix::unistd::AccessFlags::W_OK).is_ok()
}

/// Windows không có access(2): thử tạo file thật trong dir.
#[cfg(windows)]
fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".orginv-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(unix)]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(windows)]
fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Temp"))
}

/// Create the data + logs directories and pin the data dir for the process.
pub fn initialize(dir: PathBuf) -> std::io::Result<()> {
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(dir.join("logs"))?;
    let _ = DATA_DIR.set(dir);
    Ok(())
}

pub fn data_dir() -> &'static Path {
    DATA_DIR
        .get()
        .map(PathBuf::as_path)
        .expect("paths::initialize not called")
}

pub fn config_file() -> PathBuf {
    data_dir().join("config.json")
}
pub fn state_file() -> PathBuf {
    data_dir().join("state.json")
}
pub fn cache_db_file() -> PathBuf {
    data_dir().join("cache.db")
}
pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}
pub fn update_dir() -> PathBuf {
    data_dir().join("update")
}
pub fn cert_file() -> PathBuf {
    data_dir().join("client-cert.pem")
}
pub fn key_file() -> PathBuf {
    data_dir().join("client-key.pem")
}
pub fn ca_cert_file() -> PathBuf {
    data_dir().join("ca-cert.pem")
}
