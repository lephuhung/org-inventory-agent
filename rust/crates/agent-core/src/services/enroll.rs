//! Port of EnrollClient.cs + EnrollCoordinator.cs.

use crate::api::{ApiClient, ApiError};
use crate::keystore::KeyStore;
use crate::schema::FingerprintPayload;
use crate::{csr, identity, SharedConfig, SharedState};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type FingerprintFn = Arc<dyn Fn() -> FingerprintPayload + Send + Sync>;

#[derive(Debug, Serialize)]
pub struct EnrollRequest {
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub fingerprint: FingerprintPayload,
    pub csr_pem: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct EnrollResponse {
    pub machine_id: Option<String>,
    pub client_cert_pem: Option<String>,
    pub ca_cert_pem: Option<String>,
    pub renew_after: Option<String>,
    pub is_new_machine: bool,
    pub status: Option<String>,
    pub agent_server_url: Option<String>,
    pub heartbeat_interval_seconds: Option<i32>,
    pub heartbeat_jitter_seconds: Option<i32>,
    pub inventory_interval_hours: Option<i32>,
}

const RETRY_GATE: Duration = Duration::from_secs(60);

pub struct EnrollCoordinator {
    config: SharedConfig,
    api: Arc<ApiClient>,
    keystore: KeyStore,
    state: SharedState,
    fingerprint: FingerprintFn,
    gate: Mutex<(bool, Option<Instant>)>,
}

impl EnrollCoordinator {
    pub fn new(
        config: SharedConfig,
        api: Arc<ApiClient>,
        keystore: KeyStore,
        state: SharedState,
        fingerprint: FingerprintFn,
    ) -> Self {
        Self {
            config,
            api,
            keystore,
            state,
            fingerprint,
            gate: Mutex::new((false, None)),
        }
    }

    pub async fn ensure_enrolled(&self) -> bool {
        {
            let c = self.config.lock().unwrap();
            if identity::is_enrolled(&c) {
                return true;
            }
            if identity::is_reenroll_pending(&c) {
                return false;
            }
        }
        {
            let mut g = self.gate.lock().unwrap();
            if g.0 || g.1.is_some_and(|t| t.elapsed() < RETRY_GATE) {
                return false;
            }
            *g = (true, Some(Instant::now()));
        }
        let ok = self.enroll_core().await;
        self.gate.lock().unwrap().0 = false;
        ok
    }

    async fn enroll_core(&self) -> bool {
        let (token, cn) = {
            let mut c = self.config.lock().unwrap();
            let Some(token) = c.token.clone().filter(|t| !t.trim().is_empty()) else {
                log::error!(target: "EnrollCoordinator", "Chưa có enroll token — ghi config.json (field \"token\" hoặc \"enroll_token\").");
                return false;
            };
            let cn = c
                .csr_cn_placeholder
                .get_or_insert_with(|| format!("machine-{}", uuid::Uuid::new_v4()))
                .clone();
            (token, cn)
        };
        let Some(endpoint) = self.api.endpoints().current() else {
            log::error!(target: "EnrollCoordinator", "Chưa có endpoint server — truyền --endpoint hoặc ghi config.json (field \"endpoints\").");
            return false;
        };
        log::info!(target: "EnrollCoordinator", "Bắt đầu enroll tới {endpoint}...");
        let (key, csr_pem) =
            match csr::new_key_pair().and_then(|k| csr::create_csr_pem(&k, &cn).map(|p| (k, p))) {
                Ok(v) => v,
                Err(e) => {
                    log::error!(target: "EnrollCoordinator", "Tạo CSR lỗi: {e}");
                    return false;
                }
            };
        let fp = self.fingerprint.clone();
        let fingerprint = tokio::task::spawn_blocking(move || fp())
            .await
            .unwrap_or_default();
        let req = EnrollRequest {
            token,
            hostname: Some(crate::hostname()),
            fingerprint,
            csr_pem,
        };
        let resp = match self.api.post_json("/api/enroll", &req, false, 45).await {
            Ok(r) => r,
            Err(e @ ApiError::Transport(_)) | Err(e @ ApiError::Setup(_)) => {
                log::error!(target: "EnrollCoordinator", "Không kết nối được server enroll: {e}");
                return false;
            }
        };
        let parsed: Option<EnrollResponse> = if resp.ok {
            match resp
                .body
                .clone()
                .map(serde_json::from_value::<EnrollResponse>)
            {
                Some(Ok(r)) => Some(r),
                Some(Err(e)) => {
                    log::error!(target: "EnrollClient", "Parse enroll response lỗi: {e}");
                    None
                }
                None => None,
            }
        } else {
            let d = resp.detail.as_deref().unwrap_or("");
            match resp.status {
                401 => {
                    log::error!(target: "EnrollClient", "Enroll thất bại 401 (token sai/hết hạn/đã dùng/revoked): {d}")
                }
                422 => {
                    log::error!(target: "EnrollClient", "Enroll thất bại 422 (payload không hợp lệ): {d}")
                }
                429 => {
                    log::warn!(target: "EnrollClient", "Enroll bị rate-limit (429) — thử lại sau.")
                }
                s => log::error!(target: "EnrollClient", "Enroll thất bại HTTP {s}: {d}"),
            }
            None
        };
        let Some(r) = parsed.filter(|r| {
            r.machine_id
                .as_deref()
                .is_some_and(|m| !m.trim().is_empty())
                && r.client_cert_pem
                    .as_deref()
                    .is_some_and(|m| !m.trim().is_empty())
        }) else {
            log::error!(target: "EnrollCoordinator", "Enroll không thành công — thử lại ở chu kỳ sau.");
            return false;
        };
        {
            let mut c = self.config.lock().unwrap();
            if let Err(e) = self.keystore.install(
                r.client_cert_pem.as_deref().unwrap_or_default(),
                &key.serialize_pem(),
                &mut c,
                false,
            ) {
                log::error!(target: "EnrollCoordinator", "Cài client cert lỗi: {e}");
                return false;
            }
            c.machine_id = r.machine_id.clone();
            if let (Some(id), Some(cert)) = (c.machine_id.clone(), self.keystore.find(&mut c)) {
                self.keystore.mirror_legacy(&id, &cert, true);
            }
            c.enrolled = true;
            c.renew_after = r.renew_after.clone();
            c.last_enrolled_at = Some(chrono::Utc::now().fixed_offset());
            c.reenroll_required = false;
            c.apply_server_settings(
                r.agent_server_url.as_deref(),
                r.heartbeat_interval_seconds,
                r.heartbeat_jitter_seconds,
                r.inventory_interval_hours,
                None,
            );
            c.token = None;
        }
        if let Some(ca) = r.ca_cert_pem.as_deref().filter(|s| !s.trim().is_empty()) {
            let _ = std::fs::write(crate::paths::ca_cert_file(), ca);
        }
        crate::save_config(&self.config);
        // AG-P2-01 (Windows): xóa bootstrap token khỏi registry sau enroll thành công.
        #[cfg(windows)]
        crate::win_bootstrap::clear_bootstrap_token();
        self.api.reset().await;
        log::info!(
            target: "EnrollCoordinator",
            "Enroll thành công: machine_id={}, is_new={}, status={}",
            r.machine_id.as_deref().unwrap_or_default(), r.is_new_machine, r.status.as_deref().unwrap_or_default()
        );
        if let Err(e) = super::config_sync::sync(&self.config, &self.api, Some(&self.state)).await {
            log::warn!(target: "EnrollCoordinator", "Không tải được /api/agent/config sau enroll: {e}");
        }
        true
    }
}
