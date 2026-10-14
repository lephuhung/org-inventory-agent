//! Port of HeartbeatService.cs.

use super::cache::OfflineCache;
use super::enroll::EnrollCoordinator;
use super::inventory::InventoryService;
use crate::api::ApiClient;
use crate::identity::{self, EnrollStatus};
use crate::keystore::KeyStore;
use crate::{SharedConfig, SharedState};
use rand::Rng;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const CERT_CHECK_EVERY: u64 = 20;

pub struct HeartbeatService {
    pub config: SharedConfig,
    pub api: Arc<ApiClient>,
    pub enroll: Arc<EnrollCoordinator>,
    pub cache: Arc<OfflineCache>,
    pub inventory: Arc<InventoryService>,
    pub keystore: KeyStore,
    pub state: SharedState,
}

pub fn next_delay(interval: i32, jitter: i32) -> Duration {
    let jitter = jitter.min(interval - 1);
    let min = (interval - jitter).max(5) as f64;
    let max = ((interval + jitter) as f64).max(min + 1.0);
    Duration::from_secs_f64(min + rand::thread_rng().gen::<f64>() * (max - min))
}

pub fn is_identity_rejected(detail: Option<&str>) -> bool {
    detail.is_some_and(|d| {
        let l = d.to_lowercase();
        l.contains("không tồn tại") || l.contains(&"CN không hợp lệ".to_lowercase())
    })
}

#[cfg(unix)]
fn uptime_sec() -> i64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|f| f as i64)
        .unwrap_or(0)
}

/// Windows: GetTickCount64 (ms kể từ boot) — không cần crate ngoài.
#[cfg(windows)]
fn uptime_sec() -> i64 {
    unsafe extern "C" {
        fn GetTickCount64() -> u64;
    }
    (unsafe { GetTickCount64() } / 1000) as i64
}

impl HeartbeatService {
    pub async fn run(self: Arc<Self>) {
        {
            let c = self.config.lock().unwrap();
            log::info!(target: "HeartbeatService", "HeartbeatService khởi động (interval={}s, jitter={}s).", c.heartbeat_interval_seconds, c.heartbeat_jitter_seconds);
        }
        let mut cycle: u64 = 0;
        loop {
            let enrolled = identity::is_enrolled(&self.config.lock().unwrap());
            if !enrolled {
                self.enroll.ensure_enrolled().await;
            } else {
                cycle += 1;
                if cycle.is_multiple_of(CERT_CHECK_EVERY) {
                    self.check_cert();
                }
            }
            if identity::is_enrolled(&self.config.lock().unwrap()) {
                self.flush_offline_cache().await;
                self.send().await;
            }
            let (i, j) = {
                let c = self.config.lock().unwrap();
                (c.heartbeat_interval_seconds, c.heartbeat_jitter_seconds)
            };
            tokio::time::sleep(next_delay(i, j)).await;
        }
    }

    /// Cert vanished while config says enrolled → wait for a fresh token (keep machine_id).
    fn check_cert(&self) {
        let mut c = self.config.lock().unwrap();
        if identity::validate(&mut c, &self.keystore) == EnrollStatus::ReenrollRequired
            && !c.reenroll_required
        {
            log::error!(target: "HeartbeatService", "Client cert không còn trong PEM files — chờ enroll token mới để re-enroll.");
            c.enrolled = false;
            c.client_cert_thumbprint = None;
            c.reenroll_required = true;
            drop(c);
            crate::save_config(&self.config);
        }
    }

    pub async fn send_once(&self) -> bool {
        if !identity::is_enrolled(&self.config.lock().unwrap()) {
            return false;
        }
        self.send().await
    }

    async fn send(&self) -> bool {
        let ip = tokio::task::spawn_blocking(crate::primary_ip)
            .await
            .unwrap_or_else(|_| "127.0.0.1".into());
        let payload = json!({
            "logged_user": crate::current_user(),
            "uptime_sec": uptime_sec(),
            "ip": ip,
            "agent_version": crate::VERSION,
        });
        let resp = match self
            .api
            .post_json("/api/heartbeat", &payload, true, 20)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(target: "HeartbeatService", "Heartbeat không gửi được: {e}");
                return false;
            }
        };
        if !resp.ok {
            log::warn!(target: "HeartbeatService", "Heartbeat thất bại HTTP {}: {}", resp.status, resp.detail.as_deref().unwrap_or(""));
            if resp.status == 401 && is_identity_rejected(resp.detail.as_deref()) {
                log::warn!(target: "HeartbeatService", "Server không nhận identity máy này — xoá enrollment để re-enroll.");
                {
                    let mut c = self.config.lock().unwrap();
                    c.enrolled = false;
                    c.client_cert_thumbprint = None;
                    c.reenroll_required = true;
                }
                crate::save_config(&self.config);
            }
            return false;
        }
        let server_url = resp
            .str_field("server_url")
            .or_else(|| resp.str_field("agent_server_url"));
        let changed = {
            let mut c = self.config.lock().unwrap();
            let mut changed = c.apply_server_settings(
                server_url.as_deref(),
                resp.pos_int("heartbeat_interval_seconds"),
                resp.pos_int("heartbeat_jitter_seconds"),
                resp.pos_int("inventory_interval_hours"),
                resp.pos_int("renew_before_percent"),
            );
            if let Some(ra) = resp
                .str_field("renew_after")
                .filter(|s| !s.trim().is_empty())
            {
                if c.renew_after.as_deref() != Some(ra.as_str()) {
                    c.renew_after = Some(ra);
                    changed = true;
                }
            }
            if changed {
                log::info!(
                    target: "HeartbeatService",
                    "Đã cập nhật cấu hình từ heartbeat response: server={}, interval={}s, jitter={}s, inv={}h",
                    c.primary_endpoint().unwrap_or_default(), c.heartbeat_interval_seconds, c.heartbeat_jitter_seconds, c.inventory_interval_hours
                );
            }
            changed
        };
        if changed {
            crate::save_config(&self.config);
        }
        if let Some(server_hash) = resp
            .str_field("agent_config_hash")
            .filter(|h| !h.trim().is_empty())
        {
            let local = self.state.lock().unwrap().last_agent_config_hash.clone();
            if !local
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(&server_hash))
            {
                log::info!(
                    target: "HeartbeatService",
                    "Server báo hash cấu hình thay đổi ({} → {server_hash}) → gọi ConfigSync để refresh.",
                    local.as_deref().unwrap_or("(none)")
                );
                match super::config_sync::sync(&self.config, &self.api, Some(&self.state)).await {
                    Ok(true) => {
                        self.state.lock().unwrap().last_agent_config_hash = Some(server_hash)
                    }
                    Ok(false) => {}
                    Err(e) => {
                        log::warn!(target: "ConfigSyncService", "Không lấy được cấu hình từ server: {e}")
                    }
                }
            }
        }
        if resp
            .body
            .as_ref()
            .and_then(|b| b.get("rescan_requested"))
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            log::info!(target: "HeartbeatService", "Server yêu cầu rescan → chạy inventory ngay.");
            self.inventory.trigger_rescan();
        }
        true
    }

    async fn flush_offline_cache(&self) {
        let pending = self.cache.get_all();
        if pending.is_empty() {
            return;
        }
        log::info!(target: "HeartbeatService", "Flush offline cache: {} bản ghi đang chờ.", pending.len());
        for item in pending {
            match self
                .api
                .post_raw_json(&item.url, &item.body, true, 30)
                .await
            {
                Ok(r) if r.ok => {
                    self.cache.delete(item.id);
                    log::info!(target: "HeartbeatService", "Đã gửi bù offline bản ghi #{}", item.id);
                }
                Ok(_) => {
                    if self.cache.increment_attempts(item.id) {
                        self.cache.delete(item.id);
                    }
                }
                Err(e) => {
                    let drop = self.cache.increment_attempts(item.id);
                    log::warn!(target: "HeartbeatService", "Gửi bù #{} lỗi: {e}{}", item.id, if drop { " — quá số lần thử, bỏ." } else { "" });
                    if drop {
                        self.cache.delete(item.id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_within_bounds() {
        for _ in 0..200 {
            let d = next_delay(30, 8).as_secs_f64();
            assert!((22.0..=38.0).contains(&d));
            let d = next_delay(5, 4).as_secs_f64();
            assert!((5.0..=9.0).contains(&d));
        }
    }

    #[test]
    fn identity_rejection_detection() {
        assert!(is_identity_rejected(Some("Machine không tồn tại")));
        assert!(is_identity_rejected(Some("cn không hợp lệ")));
        assert!(!is_identity_rejected(Some("rate limited")));
        assert!(!is_identity_rejected(None));
    }
}
