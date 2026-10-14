//! Windows-only: bootstrap overlay từ `HKLM\SOFTWARE\OrgInventory` — port của
//! `AgentConfig.OverlayRegistryBootstrap`.
//!
//! Registry = intent MỚI NHẤT của lệnh cài (MSI/install.ps1 ghi lại mỗi lần chạy).
//! Áp dụng mỗi lần load config, override khi giá trị registry non-empty:
//! - `Endpoints`  (tách `,`/`;` → backup endpoints)
//! - `EnrollToken` → `cfg.token` (xóa khỏi registry sau enroll thành công — AG-P2-01)
//! - `HttpProxy`

use crate::config::AgentConfig;

const BOOTSTRAP_KEY: &str = r"SOFTWARE\OrgInventory";

pub fn overlay_bootstrap(cfg: &mut AgentConfig) {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(BOOTSTRAP_KEY) else {
        return;
    };
    if let Ok(endpoints) = key.get_value::<String, _>("Endpoints") {
        if !endpoints.trim().is_empty() {
            cfg.endpoints.clear();
            for e in endpoints.split([',', ';']) {
                cfg.add_backup_endpoint(e.trim());
            }
        }
    }
    if let Ok(token) = key.get_value::<String, _>("EnrollToken") {
        if !token.trim().is_empty() {
            cfg.token = Some(token.trim().to_string());
        }
    }
    if let Ok(proxy) = key.get_value::<String, _>("HttpProxy") {
        if !proxy.trim().is_empty() {
            cfg.http_proxy = Some(proxy.trim().to_string());
        }
    }
    cfg.normalize();
}

/// AG-P2-01: xóa `EnrollToken` khỏi registry sau enroll thành công — credential
/// bootstrap thực sự biến mất, không chỉ khỏi config.json.
pub fn clear_bootstrap_token() {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    match RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(BOOTSTRAP_KEY, winreg::enums::KEY_SET_VALUE)
    {
        Ok(key) => {
            if key.get_value::<String, _>("EnrollToken").is_err() {
                return;
            }
            match key.delete_value("EnrollToken") {
                Ok(()) => log::info!(
                    target: "EnrollCoordinator",
                    "AG-P2-01: Đã xóa bootstrap token khỏi Registry HKLM\\SOFTWARE\\OrgInventory\\EnrollToken."
                ),
                Err(e) => log::warn!(
                    target: "EnrollCoordinator",
                    "AG-P2-01: Không xóa được EnrollToken khỏi Registry (non-fatal): {e}"
                ),
            }
        }
        Err(_) => {}
    }
}
