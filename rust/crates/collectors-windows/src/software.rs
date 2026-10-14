//! Port of SoftwareCollector.cs — đọc HKLM Uninstall (+ WOW6432Node) + HKCU Uninstall.

use agent_core::schema::SoftwareInfo;
use std::collections::HashMap;

const HKLM_UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const HKLM_UNINSTALL_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

pub fn collect() -> Vec<SoftwareInfo> {
    let mut out: HashMap<String, SoftwareInfo> = HashMap::new();
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    scan(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        HKLM_UNINSTALL,
        false,
        &mut out,
    );
    scan(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        HKLM_UNINSTALL_WOW,
        false,
        &mut out,
    );
    scan(
        &RegKey::predef(HKEY_CURRENT_USER),
        HKLM_UNINSTALL,
        true,
        &mut out,
    );
    let mut v: Vec<_> = out.into_values().collect();
    v.sort_by(|a, b| {
        a.display_name
            .clone()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&b.display_name.clone().unwrap_or_default().to_lowercase())
    });
    v.truncate(500);
    v
}

fn scan(
    root: &winreg::RegKey,
    path: &str,
    per_user: bool,
    out: &mut HashMap<String, SoftwareInfo>,
) {
    let Ok(k) = root.open_subkey(path) else {
        return;
    };
    for sub in k.enum_keys().flatten() {
        let Ok(sk) = k.open_subkey(&sub) else {
            continue;
        };
        let name: String = sk.get_value("DisplayName").unwrap_or_default();
        if name.trim().is_empty() {
            continue;
        }
        let name = name.trim().to_string();
        let key = name.to_lowercase();
        out.entry(key).or_insert_with(|| SoftwareInfo {
            display_name: Some(name.clone()),
            name: Some(name),
            version: opt(sk.get_value::<String, _>("DisplayVersion").ok()),
            publisher: opt(sk.get_value::<String, _>("Publisher").ok()),
            install_date: opt(sk.get_value::<String, _>("InstallDate").ok()),
            uninstall_string: opt(sk.get_value::<String, _>("UninstallString").ok()),
            is_per_user: per_user,
        });
    }
}

fn opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
