//! Port of AppPaths.cs + Linux Program.cs data-dir resolution.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub const LINUX_DATA_DIR: &str = "/var/lib/orginventory";

/// `--data-dir` > `ORGINV_DATA_DIR` > `ORGINVENTORY_DATA_DIR` > /var/lib/orginventory (if it exists
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
    let system = Path::new(LINUX_DATA_DIR);
    if system.is_dir() && is_writable(system) {
        return system.to_path_buf();
    }
    home_dir().join(".local").join("share").join("OrgInventory")
}

fn is_writable(dir: &Path) -> bool {
    nix::unistd::access(dir, nix::unistd::AccessFlags::W_OK).is_ok()
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
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
