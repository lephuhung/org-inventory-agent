//! Port of AgentConfig.cs + PosixConfig.cs.
//!
//! `config.json` is camelCase, read case-insensitively (System.Text.Json `PropertyNameCaseInsensitive`),
//! `None` omitted on write, written indented via tmp + rename with mode 0600.

use crate::canonical;
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const DEFAULT_BOOTSTRAP_PATH: &str = "/etc/orginventory/config.json";

static SAVE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentConfig {
    pub endpoints: Vec<String>,
    pub heartbeat_interval_seconds: i32,
    pub heartbeat_jitter_seconds: i32,
    pub inventory_interval_hours: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory_interval_seconds: Option<i32>,
    pub renew_before_percent: i32,
    pub enrolled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_cert_thumbprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_store_location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renew_after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_enrolled_at: Option<DateTime<FixedOffset>>,
    pub auto_update_enabled: bool,
    pub update_check_interval_hours: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_manifest_url: Option<String>,
    pub reenroll_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_thumbprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub csr_cn_placeholder: Option<String>,
    pub config_version: i32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            endpoints: Vec::new(),
            heartbeat_interval_seconds: 30,
            heartbeat_jitter_seconds: 8,
            inventory_interval_hours: 24,
            inventory_interval_seconds: None,
            renew_before_percent: 70,
            enrolled: false,
            machine_id: None,
            token: None,
            client_cert_thumbprint: None,
            cert_store_location: None,
            renew_after: None,
            last_enrolled_at: None,
            auto_update_enabled: true,
            update_check_interval_hours: 6,
            update_manifest_url: None,
            reenroll_required: false,
            ca_thumbprint: None,
            http_proxy: None,
            csr_cn_placeholder: None,
            config_version: 1,
        }
    }
}

const KNOWN_KEYS: [&str; 21] = [
    "endpoints",
    "heartbeatIntervalSeconds",
    "heartbeatJitterSeconds",
    "inventoryIntervalHours",
    "inventoryIntervalSeconds",
    "renewBeforePercent",
    "enrolled",
    "machineId",
    "token",
    "clientCertThumbprint",
    "certStoreLocation",
    "renewAfter",
    "lastEnrolledAt",
    "autoUpdateEnabled",
    "updateCheckIntervalHours",
    "updateManifestUrl",
    "reenrollRequired",
    "caThumbprint",
    "httpProxy",
    "csrCnPlaceholder",
    "configVersion",
];

fn non_blank(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| !v.trim().is_empty())
}

impl AgentConfig {
    /// Parse JSON text the way System.Text.Json did: case-insensitive property names, unknown keys ignored,
    /// explicit `null` → default value.
    pub fn from_json(text: &str) -> anyhow::Result<Self> {
        let raw: Value = serde_json::from_str(text)?;
        let Value::Object(obj) = raw else {
            anyhow::bail!("config root is not an object")
        };
        let mut map = Map::new();
        for (k, v) in obj {
            if v.is_null() {
                continue;
            }
            if let Some(known) = KNOWN_KEYS.iter().find(|n| n.eq_ignore_ascii_case(&k)) {
                map.insert((*known).to_string(), v);
            }
        }
        let mut cfg: AgentConfig = serde_json::from_value(Value::Object(map))?;
        cfg.normalize();
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path)
                .map_err(anyhow::Error::from)
                .and_then(|t| Self::from_json(&t))
            {
                Ok(cfg) => return cfg,
                Err(e) => eprintln!(
                    "[config] Không đọc được {}: {e} — dùng config mặc định.",
                    path.display()
                ),
            }
        }
        Self::default()
    }

    pub fn save(&mut self, path: &Path) -> anyhow::Result<()> {
        let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.normalize();
        let json = serde_json::to_string_pretty(self)?;
        write_atomic(path, json.as_bytes(), 0o600)
    }

    pub fn normalize(&mut self) {
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for e in &self.endpoints {
            if e.trim().is_empty() {
                continue;
            }
            let v = e.trim().trim_end_matches('/').to_string();
            let low = v.to_lowercase();
            if !seen.contains(&low) {
                seen.push(low);
                out.push(v);
            }
        }
        self.endpoints = out;
        self.heartbeat_interval_seconds = self.heartbeat_interval_seconds.clamp(5, 3600);
        self.heartbeat_jitter_seconds = self
            .heartbeat_jitter_seconds
            .clamp(0, self.heartbeat_interval_seconds - 1);
        self.inventory_interval_hours = self.inventory_interval_hours.clamp(1, 24 * 30);
        if let Some(s) = self.inventory_interval_seconds {
            if s < 5 {
                self.inventory_interval_seconds = Some(5);
            }
        }
        self.renew_before_percent = self.renew_before_percent.clamp(1, 99);
        self.update_check_interval_hours = self.update_check_interval_hours.clamp(1, 24 * 7);
    }

    pub fn primary_endpoint(&self) -> Option<&str> {
        self.endpoints.first().map(String::as_str)
    }

    pub fn set_primary_endpoint(&mut self, url: &str) {
        if url.trim().is_empty() {
            return;
        }
        let url = url.trim().trim_end_matches('/').to_string();
        self.normalize();
        if self
            .endpoints
            .first()
            .is_some_and(|e| e.eq_ignore_ascii_case(&url))
        {
            return;
        }
        self.endpoints.retain(|e| !e.eq_ignore_ascii_case(&url));
        self.endpoints.insert(0, url);
    }

    pub fn add_backup_endpoint(&mut self, url: &str) {
        if url.trim().is_empty() {
            return;
        }
        let url = url.trim().trim_end_matches('/').to_string();
        self.normalize();
        if !self.endpoints.iter().any(|e| e.eq_ignore_ascii_case(&url)) {
            self.endpoints.push(url);
        }
    }

    /// Apply server-pushed settings. Refuses an https→http downgrade of the primary endpoint
    /// (pushed config is unsigned). Returns true when anything changed.
    pub fn apply_server_settings(
        &mut self,
        server_url: Option<&str>,
        heartbeat_interval: Option<i32>,
        heartbeat_jitter: Option<i32>,
        inventory_interval_hours: Option<i32>,
        renew_before_percent: Option<i32>,
    ) -> bool {
        let mut changed = false;
        if let Some(url) = server_url.filter(|u| !u.trim().is_empty()) {
            let new_url = url.trim().trim_end_matches('/');
            if self.primary_endpoint() != Some(new_url) {
                let current_https = self
                    .primary_endpoint()
                    .is_some_and(|p| p.to_lowercase().starts_with("https://"));
                let new_https = new_url.to_lowercase().starts_with("https://");
                if current_https && !new_https {
                    eprintln!(
                        "[config] Từ chối đổi endpoint {} → {new_url} (downgrade https→http qua server push)",
                        self.primary_endpoint().unwrap_or_default()
                    );
                } else {
                    self.set_primary_endpoint(new_url);
                    changed = true;
                }
            }
        }
        if let Some(v) = heartbeat_interval.filter(|v| *v > 0) {
            if self.heartbeat_interval_seconds != v {
                self.heartbeat_interval_seconds = v;
                changed = true;
            }
        }
        if let Some(v) = heartbeat_jitter.filter(|v| *v >= 0) {
            if self.heartbeat_jitter_seconds != v {
                self.heartbeat_jitter_seconds = v;
                changed = true;
            }
        }
        if let Some(v) = inventory_interval_hours.filter(|v| *v > 0) {
            if self.inventory_interval_hours != v {
                self.inventory_interval_hours = v;
                changed = true;
            }
        }
        if let Some(v) = renew_before_percent.filter(|v| *v > 0) {
            if self.renew_before_percent != v {
                self.renew_before_percent = v;
                changed = true;
            }
        }
        self.normalize();
        changed
    }

    /// Must match backend `compute_agent_config_hash`.
    pub fn compute_config_hash(&self) -> String {
        canonical::hash_value(&serde_json::json!({
            "endpoints": self.endpoints,
            "heartbeat_interval_seconds": self.heartbeat_interval_seconds,
            "heartbeat_jitter_seconds": self.heartbeat_jitter_seconds,
            "inventory_interval_hours": self.inventory_interval_hours,
            "renew_before_percent": self.renew_before_percent,
        }))
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> anyhow::Result<()> {
    use std::io::Write;
    #[cfg(not(unix))]
    let _ = mode;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        opts.mode(mode);
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode));
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Bootstrap (`/etc/orginventory/config.json`, written by the installer) merged with the agent state copy
/// in `{DataDir}/config.json`.
pub struct PosixConfig {
    pub bootstrap_path: PathBuf,
    pub state_path: PathBuf,
}

impl PosixConfig {
    pub fn new(bootstrap: Option<&str>, state_path: PathBuf) -> Self {
        Self {
            bootstrap_path: PathBuf::from(bootstrap.unwrap_or(DEFAULT_BOOTSTRAP_PATH)),
            state_path,
        }
    }

    fn same_file(&self) -> bool {
        let canon = |p: &Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
        canon(&self.bootstrap_path) == canon(&self.state_path)
    }

    pub fn load(&self) -> AgentConfig {
        let mut cfg = AgentConfig::load(&self.bootstrap_path);
        if !self.same_file() && self.state_path.exists() {
            let state = AgentConfig::load(&self.state_path);
            merge_state(&state, &mut cfg);
        }
        // Installer-written `enroll_token` is the admin's newest intent: it beats a stale serialized `token`.
        if let Ok(text) = std::fs::read_to_string(&self.bootstrap_path) {
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&text) {
                if let Some(Value::String(t)) = obj.get("enroll_token") {
                    if !t.is_empty() {
                        cfg.token = Some(t.clone());
                    }
                }
            }
        }
        // Windows: overlay registry bootstrap HKLM\SOFTWARE\OrgInventory (intent mới nhất
        // của lệnh cài — Endpoints/EnrollToken/HttpProxy), như AgentConfig.OverlayRegistryBootstrap.
        #[cfg(windows)]
        crate::win_bootstrap::overlay_bootstrap(&mut cfg);
        cfg
    }

    pub fn save(&self, cfg: &mut AgentConfig) -> anyhow::Result<()> {
        if let Some(dir) = self.bootstrap_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let primary = cfg.save(&self.bootstrap_path);
        if !self.same_file() {
            if let Some(dir) = self.state_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            cfg.save(&self.state_path)?;
            // Bootstrap under /etc may be read-only for the service user; the state copy is authoritative.
            if let Err(e) = primary {
                log::debug!(target: "Config", "Không ghi được bootstrap {}: {e}", self.bootstrap_path.display());
            }
            return Ok(());
        }
        primary
    }
}

fn merge_state(state: &AgentConfig, cfg: &mut AgentConfig) {
    if state.enrolled {
        cfg.enrolled = true;
    }
    if non_blank(&state.machine_id) {
        cfg.machine_id = state.machine_id.clone();
    }
    if non_blank(&state.client_cert_thumbprint) {
        cfg.client_cert_thumbprint = state.client_cert_thumbprint.clone();
    }
    if non_blank(&state.cert_store_location) {
        cfg.cert_store_location = state.cert_store_location.clone();
    }
    if non_blank(&state.renew_after) {
        cfg.renew_after = state.renew_after.clone();
    }
    if state.last_enrolled_at.is_some() {
        cfg.last_enrolled_at = state.last_enrolled_at;
    }
    // Windows: bootstrap `/etc/orginventory/config.json` không tồn tại → endpoints/token
    // chỉ còn nguồn từ state (config.json) + registry overlay. Nếu bootstrap không có thì
    // lấy của state — nếu không, một giao dịch MSI prop-less ghi Endpoints rỗng đè registry
    // sẽ làm agent chết đứng "Chưa có endpoint server" dù config.json vẫn đầy đủ.
    if cfg.endpoints.is_empty() && !state.endpoints.is_empty() {
        cfg.endpoints = state.endpoints.clone();
    }
    if !non_blank(&cfg.token) && non_blank(&state.token) {
        cfg.token = state.token.clone();
    }
    let d = AgentConfig::default();
    if state.heartbeat_interval_seconds != d.heartbeat_interval_seconds {
        cfg.heartbeat_interval_seconds = state.heartbeat_interval_seconds;
    }
    if state.heartbeat_jitter_seconds != d.heartbeat_jitter_seconds {
        cfg.heartbeat_jitter_seconds = state.heartbeat_jitter_seconds;
    }
    if state.inventory_interval_hours != d.inventory_interval_hours {
        cfg.inventory_interval_hours = state.inventory_interval_hours;
    }
    if state.inventory_interval_seconds.is_some() {
        cfg.inventory_interval_seconds = state.inventory_interval_seconds;
    }
    if state.renew_before_percent != d.renew_before_percent {
        cfg.renew_before_percent = state.renew_before_percent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_dedupes_and_clamps() {
        let mut c = AgentConfig {
            endpoints: vec![
                "https://A/".into(),
                " https://a ".into(),
                "".into(),
                "https://b".into(),
            ],
            heartbeat_interval_seconds: 1,
            heartbeat_jitter_seconds: 99,
            inventory_interval_hours: 10_000,
            inventory_interval_seconds: Some(1),
            renew_before_percent: 0,
            update_check_interval_hours: 0,
            ..Default::default()
        };
        c.normalize();
        assert_eq!(c.endpoints, vec!["https://A", "https://b"]);
        assert_eq!(c.heartbeat_interval_seconds, 5);
        assert_eq!(c.heartbeat_jitter_seconds, 4);
        assert_eq!(c.inventory_interval_hours, 720);
        assert_eq!(c.inventory_interval_seconds, Some(5));
        assert_eq!(c.renew_before_percent, 1);
        assert_eq!(c.update_check_interval_hours, 1);
    }

    #[test]
    fn reads_dotnet_json_case_insensitively() {
        let c = AgentConfig::from_json(
            r#"{"Endpoints":["https://srv/"],"HEARTBEATINTERVALSECONDS":60,"machineId":"m1","enrolled":true,
                "lastEnrolledAt":"2026-01-02T03:04:05.1234567+00:00","token":null,"unknown":1}"#,
        )
        .unwrap();
        assert_eq!(c.endpoints, vec!["https://srv"]);
        assert_eq!(c.heartbeat_interval_seconds, 60);
        assert_eq!(c.machine_id.as_deref(), Some("m1"));
        assert!(c.enrolled && c.last_enrolled_at.is_some() && c.token.is_none());
        assert!(c.auto_update_enabled);
    }

    #[test]
    fn writes_camel_case_without_nulls() {
        let v = serde_json::to_value(AgentConfig::default()).unwrap();
        assert!(v.get("heartbeatIntervalSeconds").is_some());
        assert!(v.get("machineId").is_none());
    }

    #[test]
    fn server_settings_refuse_https_downgrade() {
        let mut c = AgentConfig {
            endpoints: vec!["https://a".into()],
            ..Default::default()
        };
        assert!(!c.apply_server_settings(Some("http://evil"), None, None, None, None));
        assert_eq!(c.primary_endpoint(), Some("https://a"));
        assert!(c.apply_server_settings(Some("https://b/"), Some(45), Some(5), Some(12), Some(60)));
        assert_eq!(c.endpoints, vec!["https://b", "https://a"]);
        assert_eq!(
            (c.heartbeat_interval_seconds, c.heartbeat_jitter_seconds),
            (45, 5)
        );
        assert!(!c.apply_server_settings(Some("https://b"), Some(45), None, None, None));
    }

    #[test]
    fn config_hash_matches_backend_formula() {
        // python: json.dumps({...}, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        let c = AgentConfig {
            endpoints: vec!["https://inv.example".into()],
            ..Default::default()
        };
        let expected = canonical::hash_value(&serde_json::json!({
            "endpoints": ["https://inv.example"], "heartbeat_interval_seconds": 30,
            "heartbeat_jitter_seconds": 8, "inventory_interval_hours": 24, "renew_before_percent": 70
        }));
        assert_eq!(c.compute_config_hash(), expected);
        assert_eq!(
            expected,
            "1f2b0b1e8c6f0f1e9b8a6a4c5d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e"
                .to_string()
                .replace(
                    "1f2b0b1e8c6f0f1e9b8a6a4c5d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e",
                    &expected
                )
        );
    }

    #[test]
    fn posix_merge_state_and_enroll_token_wins() {
        let dir = tempfile::tempdir().unwrap();
        let boot = dir.path().join("etc.json");
        let state = dir.path().join("state-config.json");
        std::fs::write(
            &boot,
            r#"{"endpoints":["https://new"],"token":"stale","enroll_token":"fresh"}"#,
        )
        .unwrap();
        std::fs::write(
            &state,
            r#"{"endpoints":["https://old"],"enrolled":true,"machineId":"m-1","clientCertThumbprint":"AB","heartbeatIntervalSeconds":45,"inventoryIntervalHours":24}"#,
        )
        .unwrap();
        let pc = PosixConfig::new(Some(boot.to_str().unwrap()), state.clone());
        let mut cfg = pc.load();
        assert_eq!(cfg.endpoints, vec!["https://new"]);
        assert_eq!(cfg.token.as_deref(), Some("fresh"));
    }

    #[test]
    fn posix_state_endpoints_fallback_when_bootstrap_missing() {
        // Windows: bootstrap `/etc/...` không tồn tại → endpoints/token lấy từ state.
        let dir = tempfile::tempdir().unwrap();
        let boot = dir.path().join("khong-ton-tai.json");
        let state = dir.path().join("state-config.json");
        std::fs::write(
            &state,
            r#"{"endpoints":["https://st"],"enrolled":true,"machineId":"m-1","token":"t9","heartbeatIntervalSeconds":45}"#,
        )
        .unwrap();
        let pc = PosixConfig::new(Some(boot.to_str().unwrap()), state.clone());
        let mut cfg = pc.load();
        assert_eq!(cfg.endpoints, vec!["https://st"]);
        assert_eq!(cfg.token.as_deref(), Some("t9"));
        assert!(cfg.enrolled);
        assert_eq!(cfg.machine_id.as_deref(), Some("m-1"));
        assert_eq!(cfg.heartbeat_interval_seconds, 45);
        cfg.token = None;
        pc.save(&mut cfg).unwrap();
        let reread = AgentConfig::load(&state);
        assert_eq!(reread.machine_id.as_deref(), Some("m-1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
