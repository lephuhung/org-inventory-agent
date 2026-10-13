//! Port of ConfigSyncService.cs — GET /api/agent/config every 6 h (and on hash change).

use crate::api::ApiClient;
use crate::{identity, SharedConfig, SharedState};
use std::sync::Arc;
use std::time::Duration;

pub const SYNC_INTERVAL: Duration = Duration::from_secs(6 * 3600);

/// Apply server settings; when `state` is given also persist `agent_config_hash` (SyncAndSaveHashAsync).
pub async fn sync(
    config: &SharedConfig,
    api: &ApiClient,
    state: Option<&SharedState>,
) -> anyhow::Result<bool> {
    let resp = api.get_json("/api/agent/config", true, 30).await?;
    if !resp.ok {
        log::warn!(target: "ConfigSyncService", "GET /api/agent/config thất bại HTTP {}: {}", resp.status, resp.detail.as_deref().unwrap_or(""));
        return Ok(false);
    }
    if resp.body.is_none() {
        return Ok(false);
    }
    let changed = config.lock().unwrap().apply_server_settings(
        resp.str_field("server_url").as_deref(),
        resp.pos_int("heartbeat_interval_seconds"),
        resp.pos_int("heartbeat_jitter_seconds"),
        resp.pos_int("inventory_interval_hours"),
        resp.pos_int("renew_before_percent"),
    );
    if changed {
        crate::save_config(config);
        let c = config.lock().unwrap();
        log::info!(
            target: "ConfigSyncService",
            "Đã đồng bộ cấu hình từ server: server={}, interval={}s, jitter={}s, inventory={}h, renew={}%",
            c.primary_endpoint().unwrap_or_default(), c.heartbeat_interval_seconds, c.heartbeat_jitter_seconds,
            c.inventory_interval_hours, c.renew_before_percent
        );
    }
    if let (Some(state), Some(hash)) = (
        state,
        resp.str_field("agent_config_hash")
            .filter(|h| !h.trim().is_empty()),
    ) {
        let mut s = state.lock().unwrap();
        s.last_agent_config_hash = Some(hash.clone());
        s.save();
        log::info!(target: "ConfigSyncService", "Đã cập nhật LastAgentConfigHash={hash}");
    }
    Ok(true)
}

pub async fn run(
    config: SharedConfig,
    api: Arc<ApiClient>,
    enroll: Arc<super::enroll::EnrollCoordinator>,
) {
    while !identity::is_enrolled(&config.lock().unwrap()) {
        enroll.ensure_enrolled().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
    loop {
        if identity::is_enrolled(&config.lock().unwrap()) {
            if let Err(e) = sync(&config, &api, None).await {
                log::warn!(target: "ConfigSyncService", "Không lấy được cấu hình từ server: {e}");
            }
        }
        tokio::time::sleep(SYNC_INTERVAL).await;
    }
}
