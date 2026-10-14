//! Port of StartupCollector.cs — Run keys (HKLM, HKLM WOW6432Node, HKCU).

use agent_core::schema::StartupProgramInfo;
use std::collections::HashMap;

pub fn collect() -> Vec<StartupProgramInfo> {
    let mut out: HashMap<String, StartupProgramInfo> = HashMap::new();
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    scan(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "HKLM_Run",
        &mut out,
    );
    scan(
        &RegKey::predef(HKEY_LOCAL_MACHINE),
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        "HKLM_Run64",
        &mut out,
    );
    scan(
        &RegKey::predef(HKEY_CURRENT_USER),
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "HKCU_Run",
        &mut out,
    );
    let mut v: Vec<_> = out.into_values().collect();
    v.sort_by(|a, b| {
        a.name
            .clone()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&b.name.clone().unwrap_or_default().to_lowercase())
    });
    v.truncate(50);
    v
}

fn scan(
    root: &winreg::RegKey,
    path: &str,
    location: &str,
    out: &mut HashMap<String, StartupProgramInfo>,
) {
    let Ok(k) = root.open_subkey(path) else {
        return;
    };
    for name in k.enum_values().flatten().map(|(n, _)| n) {
        if name.trim().is_empty() {
            continue;
        }
        let cmd: String = k.get_value(&name).unwrap_or_default();
        out.entry(name.to_lowercase())
            .or_insert_with(|| StartupProgramInfo {
                name: Some(name.trim().to_string()),
                command: Some(cmd),
                location: Some(location.to_string()),
            });
    }
}
