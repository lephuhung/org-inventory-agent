//! Port of InventoryService.cs.

use super::cache::OfflineCache;
use super::enroll::EnrollCoordinator;
use crate::api::ApiClient;
use crate::payload::{self, InventoryProvider};
use crate::{identity, SharedConfig, SharedState};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct InventoryService {
    pub config: SharedConfig,
    pub api: Arc<ApiClient>,
    pub enroll: Arc<EnrollCoordinator>,
    pub cache: Arc<OfflineCache>,
    pub provider: Arc<dyn InventoryProvider>,
    pub state: SharedState,
    rescan: AtomicBool,
    sending: AtomicBool,
}

impl InventoryService {
    pub fn new(
        config: SharedConfig,
        api: Arc<ApiClient>,
        enroll: Arc<EnrollCoordinator>,
        cache: Arc<OfflineCache>,
        provider: Arc<dyn InventoryProvider>,
        state: SharedState,
    ) -> Self {
        Self {
            config,
            api,
            enroll,
            cache,
            provider,
            state,
            rescan: AtomicBool::new(false),
            sending: AtomicBool::new(false),
        }
    }

    pub fn trigger_rescan(&self) {
        self.rescan.store(true, Ordering::SeqCst);
    }

    pub async fn run(self: Arc<Self>) {
        while !identity::is_enrolled(&self.config.lock().unwrap()) {
            self.enroll.ensure_enrolled().await;
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
        let desc = {
            let c = self.config.lock().unwrap();
            match c.inventory_interval_seconds.filter(|s| *s > 0) {
                Some(s) => format!("{s}s"),
                None => format!("{}h", c.inventory_interval_hours),
            }
        };
        log::info!(target: "InventoryService", "InventoryService khởi động (interval={desc}).");
        loop {
            if identity::is_enrolled(&self.config.lock().unwrap()) && self.is_due() {
                self.rescan.store(false, Ordering::SeqCst);
                self.send().await;
            }
            let delay = self
                .config
                .lock()
                .unwrap()
                .inventory_interval_seconds
                .filter(|s| *s > 0)
                .map_or(30, |s| s.clamp(5, 30));
            tokio::time::sleep(Duration::from_secs(delay as u64)).await;
        }
    }

    fn is_due(&self) -> bool {
        if self.rescan.load(Ordering::SeqCst) {
            return true;
        }
        let (hash, interval) = {
            let c = self.config.lock().unwrap();
            let interval = match c.inventory_interval_seconds.filter(|s| *s > 0) {
                Some(s) => chrono::Duration::seconds(s as i64),
                None => chrono::Duration::hours(c.inventory_interval_hours.max(1) as i64),
            };
            (c.compute_config_hash(), interval)
        };
        let st = self.state.lock().unwrap();
        if st.last_inventory_config_hash.as_deref() != Some(hash.as_str()) {
            return true;
        }
        match st.last_inventory_at {
            None => true,
            Some(last) => chrono::Utc::now().signed_duration_since(last) >= interval,
        }
    }

    pub async fn send_once(&self) -> bool {
        if !identity::is_enrolled(&self.config.lock().unwrap()) {
            return false;
        }
        self.send().await
    }

    async fn send(&self) -> bool {
        if self.sending.swap(true, Ordering::SeqCst) {
            return false;
        }
        let ok = self.send_inner().await;
        self.sending.store(false, Ordering::SeqCst);
        ok
    }

    async fn send_inner(&self) -> bool {
        let config_hash = self.config.lock().unwrap().compute_config_hash();
        let provider = self.provider.clone();
        let user = crate::current_user();
        let body =
            match tokio::task::spawn_blocking(move || payload::build(provider.as_ref(), &user))
                .await
            {
                Ok(b) => b,
                Err(e) => {
                    log::error!(target: "InventoryService", "Thu thập inventory lỗi: {e}");
                    return false;
                }
            };
        let url = match self.api.endpoints().build_url("/api/inventory") {
            Ok(u) => u,
            Err(e) => {
                log::error!(target: "InventoryService", "{e}");
                return false;
            }
        };
        match self.api.post_json("/api/inventory", &body, true, 60).await {
            Ok(r) if r.ok => {
                {
                    let mut st = self.state.lock().unwrap();
                    st.last_inventory_at = Some(chrono::Utc::now().fixed_offset());
                    st.last_inventory_config_hash = Some(config_hash);
                    st.save();
                }
                let changed = r
                    .body
                    .as_ref()
                    .and_then(|b| b.get("config_changed"))
                    .and_then(|v| v.as_bool());
                log::info!(target: "InventoryService", "Đã gửi inventory (config_changed={}).", changed.map_or("".into(), |b| b.to_string()));
                true
            }
            Ok(r) => {
                log::warn!(target: "InventoryService", "Inventory thất bại HTTP {}: {} → lưu offline cache.", r.status, r.detail.as_deref().unwrap_or(""));
                self.enqueue(&url, &body);
                false
            }
            Err(e) => {
                log::warn!(target: "InventoryService", "Inventory không gửi được: {e} → lưu offline cache.");
                self.enqueue(&url, &body);
                false
            }
        }
    }

    fn enqueue(&self, url: &str, body: &serde_json::Value) {
        match serde_json::to_string(body) {
            Ok(text) => self.cache.enqueue(url, &text),
            Err(e) => {
                log::error!(target: "InventoryService", "Lưu offline cache inventory lỗi: {e}")
            }
        }
    }
}
