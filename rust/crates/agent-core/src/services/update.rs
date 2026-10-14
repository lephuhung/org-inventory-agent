//! Port of UpdateService.cs (Linux path): manifest → download `OrgInventoryAgent-<rid>` + `.sha256`
//! sidecar → stage in `{DataDir}/update/` → write `update.pending` for orginventory-agent-update.path.

use crate::{SharedConfig, SharedState};
use rand::Rng;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/lephuhung/org-inventory-agent/releases/latest/download/agent-version.json";
pub const PENDING_MARKER: &str = "update.pending";
const INITIAL_DELAY: Duration = Duration::from_secs(90);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const APPLY_SCRIPT: &str = "/opt/orginventory/apply-update.sh";

// ---- UpdateManifest -------------------------------------------------------------------------

pub fn select_version(manifest_json: &str, rid: &str) -> Option<String> {
    let v: Value = serde_json::from_str(manifest_json).ok()?;
    let obj = v.as_object()?;
    if rid.starts_with("win") {
        return obj.get("msi_version")?.as_str().map(str::to_string);
    }
    let section = if rid.starts_with("osx") {
        "osx"
    } else {
        "linux"
    };
    obj.get(section)?
        .as_object()?
        .get(rid)?
        .as_str()
        .map(str::to_string)
}

pub fn asset_name(rid: &str) -> String {
    if rid.starts_with("win") {
        "OrgInventoryAgent.msi".into()
    } else {
        format!("OrgInventoryAgent-{rid}")
    }
}

pub fn staged_name(rid: &str) -> &'static str {
    if rid.starts_with("win") {
        "OrgInventoryAgent.msi"
    } else {
        "OrgInventoryAgent"
    }
}

/// System.Version semantics: 2–4 numeric parts, missing parts compare as -1 (1.2 < 1.2.0).
pub fn parse_version(text: &str) -> Option<[i64; 4]> {
    let t = text.trim().trim_start_matches(['v', 'V']);
    let t = t.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = t.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut out = [-1i64; 4];
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out[i] = p.parse().ok()?;
    }
    Some(out)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

pub fn current_rid() -> String {
    if cfg!(target_os = "macos") {
        return if std::env::consts::ARCH == "aarch64" {
            "osx-arm64"
        } else {
            "osx-x64"
        }
        .into();
    }
    let musl = ["/lib", "/usr/lib"].iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|rd| {
            rd.flatten().any(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with("ld-musl-") && n.ends_with(".so.1")
            })
        })
    });
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    format!("linux{}-{arch}", if musl { "-musl" } else { "" })
}

// ---- Service --------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub pending_version: Option<String>,
    pub error: Option<String>,
}

pub struct UpdateService {
    config: SharedConfig,
    state: SharedState,
    http: reqwest::Client,
    update_dir: PathBuf,
    current_version: String,
}

impl UpdateService {
    pub fn new(config: SharedConfig, state: SharedState) -> anyhow::Result<Self> {
        let proxy = config.lock().unwrap().http_proxy.clone();
        let mut b =
            reqwest::Client::builder().user_agent(format!("OrgInventoryAgent/{}", crate::VERSION));
        if let Some(p) = proxy.filter(|p| !p.trim().is_empty()) {
            b = b.proxy(reqwest::Proxy::all(p.trim())?);
        }
        Ok(Self {
            config,
            state,
            http: b.build()?,
            update_dir: crate::paths::update_dir(),
            current_version: crate::VERSION.into(),
        })
    }

    pub fn with_dir(mut self, dir: PathBuf, current_version: &str) -> Self {
        self.update_dir = dir;
        self.current_version = current_version.into();
        self
    }

    pub async fn run(self) {
        if !self.config.lock().unwrap().auto_update_enabled {
            log::info!(target: "UpdateService", "Auto-update tắt (autoUpdateEnabled=false).");
            return;
        }
        tokio::time::sleep(INITIAL_DELAY).await;
        loop {
            self.check_and_apply().await;
            let hours = self
                .config
                .lock()
                .unwrap()
                .update_check_interval_hours
                .clamp(1, 24 * 7) as f64;
            let jitter = 1.0 + (rand::thread_rng().gen::<f64>() - 0.5) * 0.5;
            tokio::time::sleep(Duration::from_secs_f64(hours * jitter * 3600.0)).await;
        }
    }

    fn manifest_url(&self) -> String {
        self.config
            .lock()
            .unwrap()
            .update_manifest_url
            .clone()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_MANIFEST_URL.into())
    }

    pub async fn check(&self) -> UpdateCheckResult {
        let pending = self.read_pending_version();
        match self.fetch_text(&self.manifest_url(), HTTP_TIMEOUT).await {
            None => UpdateCheckResult {
                current_version: self.current_version.clone(),
                latest_version: None,
                update_available: false,
                pending_version: pending,
                error: Some("Không tải được manifest.".into()),
            },
            Some(m) => {
                let latest = select_version(&m, &current_rid());
                let newer = latest
                    .as_deref()
                    .is_some_and(|l| is_newer(l, &self.current_version));
                UpdateCheckResult {
                    current_version: self.current_version.clone(),
                    latest_version: latest,
                    update_available: newer,
                    pending_version: pending,
                    error: None,
                }
            }
        }
    }

    pub async fn check_and_apply(&self) -> bool {
        let manifest_url = self.manifest_url();
        let manifest = self.fetch_text(&manifest_url, HTTP_TIMEOUT).await;
        {
            let mut st = self.state.lock().unwrap();
            st.last_update_check_at = Some(chrono::Utc::now().fixed_offset());
            st.save();
        }
        let Some(manifest) = manifest else {
            return false;
        };
        let rid = current_rid();
        let Some(latest) = select_version(&manifest, &rid) else {
            log::debug!(target: "UpdateService", "Manifest không có entry cho RID {rid}.");
            return false;
        };
        if !is_newer(&latest, &self.current_version) {
            log::debug!(target: "UpdateService", "Đã ở bản mới nhất ({} >= {latest}).", self.current_version);
            self.cleanup_staging_if_idle();
            return false;
        }
        if let Some(p) = self.read_pending_version() {
            if !is_newer(&latest, &p) {
                log::info!(target: "UpdateService", "Bản {p} đã stage, đang chờ apply.");
                self.ensure_pending_marker(&p);
                return true;
            }
        }
        let base = &manifest_url[..manifest_url.rfind('/').unwrap_or(manifest_url.len())];
        let asset_url = format!("{base}/{}", asset_name(&rid));
        let sha_url = format!("{asset_url}.sha256");
        log::info!(target: "UpdateService", "Có phiên bản mới {latest} (đang chạy {}) — tải {asset_url}...", self.current_version);
        let _ = std::fs::create_dir_all(&self.update_dir);
        self.cleanup_staging_if_idle();
        let staged_file = staged_name(&rid);
        let staged = self.update_dir.join(staged_file);
        let tmp = self.update_dir.join(format!("{staged_file}.part"));
        let result: anyhow::Result<bool> = async {
            self.download(&asset_url, &tmp).await?;
            let Some(expected) = self.fetch_text(&sha_url, HTTP_TIMEOUT).await.filter(|s| !s.trim().is_empty()) else {
                log::warn!(target: "UpdateService", "Thiếu sidecar SHA-256 ({sha_url}) — hủy cập nhật.");
                return Ok(false);
            };
            let expected = expected.split_whitespace().next().unwrap_or("").to_string();
            let actual = hex::encode(Sha256::digest(std::fs::read(&tmp)?));
            if !expected.eq_ignore_ascii_case(&actual) {
                log::warn!(target: "UpdateService", "SHA-256 không khớp (server={expected}, file={actual}) — hủy cập nhật.");
                return Ok(false);
            }
            std::fs::rename(&tmp, &staged)?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
            std::fs::write(self.update_dir.join(format!("{staged_file}.version")), format!("{latest}\n"))?;
            std::fs::write(self.update_dir.join(format!("{staged_file}.sha256")), format!("{actual}  {staged_file}\n"))?;
            Ok(true)
        }
        .await;
        let _ = std::fs::remove_file(&tmp);
        match result {
            Ok(true) => {}
            Ok(false) => return false,
            Err(e) => {
                log::warn!(target: "UpdateService", "Tải bản mới thất bại: {e}");
                return false;
            }
        }
        let size = std::fs::metadata(&staged).map(|m| m.len()).unwrap_or(0);
        log::info!(target: "UpdateService", "Đã tải + verify {staged_file} v{latest} ({size} bytes).");
        self.stage_linux_update(&latest);
        true
    }

    fn stage_linux_update(&self, version: &str) {
        if let Err(e) = std::fs::write(self.update_dir.join(PENDING_MARKER), format!("{version}\n"))
        {
            log::warn!(target: "UpdateService", "Không ghi được pending marker: {e}");
            return;
        }
        if find_updater_unit().is_some() {
            log::info!(target: "UpdateService", "Đã stage v{version} — orginventory-agent-update.path sẽ áp dụng.");
            return;
        }
        if nix::unistd::geteuid().is_root() && Path::new(APPLY_SCRIPT).exists() {
            match std::process::Command::new("/bin/bash")
                .arg(APPLY_SCRIPT)
                .spawn()
            {
                Ok(_) => {
                    log::info!(target: "UpdateService", "Chạy apply-update.sh trực tiếp (root).");
                    return;
                }
                Err(e) => {
                    log::warn!(target: "UpdateService", "Không chạy được apply-update.sh: {e}")
                }
            }
        }
        log::warn!(
            target: "UpdateService",
            "Update v{version} đã stage tại {} nhưng thiếu systemd unit orginventory-agent-update.path — cài lại bằng install script bản mới để bật auto-apply.",
            self.update_dir.display()
        );
    }

    fn ensure_pending_marker(&self, version: &str) {
        let p = self.update_dir.join(PENDING_MARKER);
        if !p.exists() {
            let _ = std::fs::write(p, format!("{version}\n"));
        }
    }

    pub fn read_pending_version(&self) -> Option<String> {
        std::fs::read_to_string(self.update_dir.join(PENDING_MARKER))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn cleanup_staging_if_idle(&self) {
        if self.update_dir.join(PENDING_MARKER).exists() {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(&self.update_dir) {
            for e in rd.flatten() {
                if e.path().is_file() && !e.file_name().to_string_lossy().ends_with(PENDING_MARKER)
                {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    async fn fetch_text(&self, url: &str, timeout: Duration) -> Option<String> {
        match self.http.get(url).timeout(timeout).send().await {
            Ok(r) if r.status().is_success() => r.text().await.ok(),
            Ok(r) => {
                log::warn!(target: "UpdateService", "GET {url} → HTTP {}", r.status().as_u16());
                None
            }
            Err(e) if e.is_timeout() => {
                log::warn!(target: "UpdateService", "GET {url} timeout.");
                None
            }
            Err(e) => {
                log::warn!(target: "UpdateService", "GET {url} lỗi: {e}");
                None
            }
        }
    }

    async fn download(&self, url: &str, path: &Path) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;
        let mut resp = self
            .http
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await?
            .error_for_status()?;
        let mut f = tokio::fs::File::create(path).await?;
        while let Some(chunk) = resp.chunk().await? {
            f.write_all(&chunk).await?;
        }
        f.flush().await?;
        Ok(())
    }
}

fn find_updater_unit() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        // launchd WatchPaths on update.pending (com.orginventory.agent.update).
        let p = Path::new("/Library/LaunchDaemons/com.orginventory.agent.update.plist");
        return p.exists().then(|| p.to_path_buf());
    }
    [
        "/etc/systemd/system",
        "/usr/lib/systemd/system",
        "/lib/systemd/system",
    ]
    .iter()
    .map(|d| Path::new(d).join("orginventory-agent-update.path"))
    .find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_selection() {
        let m = r#"{"version":"1.4.0","msi_version":"1.4.0","linux":{"linux-x64":"1.4.0","linux-arm64":"1.3.9"},"osx":{"osx-arm64":"1.4.1"}}"#;
        assert_eq!(select_version(m, "linux-x64").as_deref(), Some("1.4.0"));
        assert_eq!(select_version(m, "linux-arm64").as_deref(), Some("1.3.9"));
        assert_eq!(select_version(m, "linux-musl-x64"), None);
        assert_eq!(select_version(m, "osx-arm64").as_deref(), Some("1.4.1"));
        assert_eq!(select_version(m, "win-x64").as_deref(), Some("1.4.0"));
        assert_eq!(select_version("not json", "linux-x64"), None);
        assert_eq!(asset_name("linux-x64"), "OrgInventoryAgent-linux-x64");
        assert_eq!(staged_name("linux-x64"), "OrgInventoryAgent");
    }

    #[test]
    fn version_compare() {
        assert!(is_newer("1.2.4", "1.2.3"));
        assert!(is_newer("v1.10.0", "1.9.9"));
        assert!(!is_newer("1.2.3", "1.2.3"));
        assert!(is_newer("1.2.0", "1.2"));
        assert!(!is_newer("1.2.3-beta", "1.2.3"));
        assert!(!is_newer("garbage", "1.0.0"));
        assert!(!is_newer("1", "0.9"));
    }
}
