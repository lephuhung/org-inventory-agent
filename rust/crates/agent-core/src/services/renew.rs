//! Port of RenewService.cs — renew at `renew_after` or when remaining life < RenewBeforePercent.

use super::enroll::EnrollCoordinator;
use crate::api::ApiClient;
use crate::keystore::KeyStore;
use crate::{csr, identity, SharedConfig};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);

pub fn remaining_life_percent(
    not_before: DateTime<Utc>,
    not_after: DateTime<Utc>,
    now: DateTime<Utc>,
) -> f64 {
    let total = (not_after - not_before).num_milliseconds() as f64;
    if total <= 0.0 {
        return 0.0;
    }
    let remaining = (not_after - now).num_milliseconds() as f64;
    (remaining / total * 100.0).clamp(0.0, 100.0)
}

pub fn parse_renew_after(s: Option<&str>) -> Option<DateTime<Utc>> {
    let s = s?.trim();
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|n| n.and_utc())
        })
}

pub struct RenewService {
    pub config: SharedConfig,
    pub api: Arc<ApiClient>,
    pub enroll: Arc<EnrollCoordinator>,
    pub keystore: KeyStore,
}

impl RenewService {
    pub async fn run(self: Arc<Self>) {
        while !identity::is_enrolled(&self.config.lock().unwrap()) {
            let (pending, mid) = {
                let c = self.config.lock().unwrap();
                (identity::is_reenroll_pending(&c), c.machine_id.clone())
            };
            if pending {
                log::info!(
                    target: "RenewService",
                    "Cert biến mất; chờ admin issue fresh bootstrap token để re-enroll (state: REENROLL_REQUIRED, machine_id={}).",
                    mid.unwrap_or_default()
                );
                tokio::time::sleep(Duration::from_secs(300)).await;
                continue;
            }
            self.enroll.ensure_enrolled().await;
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
        loop {
            if identity::is_enrolled(&self.config.lock().unwrap()) {
                self.check_and_renew().await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    }

    async fn check_and_renew(&self) {
        let (cert, renew_after, threshold) = {
            let mut c = self.config.lock().unwrap();
            if c.reenroll_required {
                return;
            }
            let cert = self.keystore.find(&mut c);
            if cert.is_none() {
                log::error!(
                    target: "RenewService",
                    "Client cert không tìm thấy. Có thể PEM file bị xóa hoặc store bị corrupt. Đặt trạng thái REENROLL_REQUIRED. Agent sẽ KHÔNG tự enroll; chờ admin issue fresh bootstrap token."
                );
                c.enrolled = false;
                c.client_cert_thumbprint = None;
                c.reenroll_required = true;
                drop(c);
                crate::save_config(&self.config);
                return;
            }
            (cert.unwrap(), c.renew_after.clone(), c.renew_before_percent)
        };
        let now = Utc::now();
        if parse_renew_after(renew_after.as_deref()).is_some_and(|ra| now >= ra) {
            log::info!(target: "RenewService", "Đến hạn renew (renew_after={}).", renew_after.unwrap_or_default());
            self.renew().await;
            return;
        }
        let remaining = remaining_life_percent(cert.not_before, cert.not_after, now);
        if remaining < threshold as f64 {
            log::info!(target: "RenewService", "Cert còn {remaining:.1}% vòng đời (< {threshold}%) → renew.");
            self.renew().await;
        }
    }

    pub async fn renew(&self) -> bool {
        let mid = self
            .config
            .lock()
            .unwrap()
            .machine_id
            .clone()
            .unwrap_or_default();
        let (key, csr_pem) = match csr::new_key_pair()
            .and_then(|k| csr::create_csr_pem(&k, &format!("machine-{mid}")).map(|p| (k, p)))
        {
            Ok(v) => v,
            Err(e) => {
                log::error!(target: "RenewService", "Tạo CSR lỗi: {e}");
                return false;
            }
        };
        let resp = match self
            .api
            .post_json("/api/renew", &json!({ "csr_pem": csr_pem }), true, 45)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(target: "RenewService", "Không gọi được /api/renew: {e}");
                return false;
            }
        };
        if !resp.ok {
            log::error!(target: "RenewService", "Renew thất bại HTTP {}: {}", resp.status, resp.detail.as_deref().unwrap_or(""));
            return false;
        }
        let Some(cert_pem) = resp
            .str_field("client_cert_pem")
            .filter(|s| !s.trim().is_empty())
        else {
            log::error!(target: "RenewService", "Renew response thiếu client_cert_pem.");
            return false;
        };
        {
            let mut c = self.config.lock().unwrap();
            if let Err(e) = self
                .keystore
                .install(&cert_pem, &key.serialize_pem(), &mut c, true)
            {
                log::error!(target: "RenewService", "Cert mới KHÔNG cài được (atomic swap fail). Cert cũ vẫn còn — sẽ retry ở chu kỳ sau. {e}");
                return false;
            }
            if let Some(ra) = resp.str_field("renew_after") {
                c.renew_after = Some(ra);
            }
        }
        crate::save_config(&self.config);
        self.api.reset().await;
        log::info!(
            target: "RenewService",
            "Renew thành công — cert mới thumbprint={}.",
            self.config.lock().unwrap().client_cert_thumbprint.clone().unwrap_or_default()
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_percent() {
        let nb = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .to_utc();
        let na = DateTime::parse_from_rfc3339("2026-01-11T00:00:00Z")
            .unwrap()
            .to_utc();
        let now = DateTime::parse_from_rfc3339("2026-01-08T00:00:00Z")
            .unwrap()
            .to_utc();
        assert!((remaining_life_percent(nb, na, now) - 30.0).abs() < 1e-9);
        assert_eq!(
            remaining_life_percent(nb, na, na + chrono::Duration::days(1)),
            0.0
        );
        assert_eq!(remaining_life_percent(na, nb, now), 0.0);
    }

    #[test]
    fn renew_after_parsing() {
        assert!(parse_renew_after(Some("2026-01-01T00:00:00+00:00")).is_some());
        assert!(parse_renew_after(Some("2026-01-01T00:00:00.123456")).is_some());
        assert!(parse_renew_after(Some("garbage")).is_none());
        assert!(parse_renew_after(None).is_none());
    }
}
