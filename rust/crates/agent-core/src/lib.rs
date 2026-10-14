//! Platform-neutral core of the OrgInventory agent (port of OrgInventoryAgent.Core).

pub mod api;
pub mod bundle;
pub mod canonical;
pub mod cli;
pub mod config;
pub mod csr;
pub mod endpoint;
pub mod identity;
pub mod keystore;
pub mod logging;
pub mod paths;
pub mod payload;
#[cfg(unix)]
pub mod posix;
pub mod schema;
pub mod services;
pub mod state;
#[cfg(windows)]
pub mod win_bootstrap;

use std::sync::{Arc, Mutex};

pub type SharedConfig = Arc<Mutex<config::AgentConfig>>;
pub type SharedState = Arc<Mutex<state::AgentState>>;

/// Version baked in by the release pipeline (`ORGINV_VERSION`), else the crate version.
pub const VERSION: &str = match option_env!("ORGINV_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub const APP_NAME: &str = "OrgInventory Agent";
pub const FULL_TITLE: &str =
    "Hệ thống Quản lý Tài sản Công nghệ Thông tin & Đánh giá An toàn Thông tin";
pub const DEVELOPER_SHORT: &str = "Phòng ANM & PCTP sử dụng CNC, Công an Hà Tĩnh";

/// Persist the agent copy of the config (`{DataDir}/config.json`), as `_config.Save()` did.
pub fn save_config(cfg: &SharedConfig) {
    let mut c = cfg.lock().unwrap();
    if let Err(e) = c.save(&paths::config_file()) {
        log::warn!(target: "Config", "Không lưu được config: {e}");
    }
}

#[cfg(unix)]
pub fn hostname() -> String {
    nix::unistd::gethostname()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(windows)]
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(unix)]
pub fn current_user() -> String {
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.name)
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default()
}

/// Environment.UserName on Windows — service chạy LocalSystem vẫn có USERNAME=SYSTEM
/// (GetUserName trả SYSTEM; env thường khớp). Fallback "SYSTEM" cho context service.
#[cfg(windows)]
pub fn current_user() -> String {
    std::env::var("USERNAME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "SYSTEM".into())
}

/// IP outbound của interface active — dùng cho heartbeat `ip`.
#[cfg(unix)]
pub fn primary_ip() -> String {
    crate::posix::network::primary_ip()
}

/// Windows: UDP connect (không gửi gói) để kernel chọn source IP —
/// tương đương "IPv4 đầu tiên của interface đang up, không loopback".
#[cfg(windows)]
pub fn primary_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}
