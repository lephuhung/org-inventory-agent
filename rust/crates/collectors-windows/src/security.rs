//! Port of SecurityCollector.cs — SecurityPostureV4 Windows.
//! WMI root\SecurityCenter2 (AV), Win32_QuickFixEngineering (WU),
//! MicrosoftVolumeEncryption (BitLocker) + registry checks; non-fatal như .NET.

use crate::wmi;
use agent_core::schema::{
    AntivirusInfo, DiskEncryptionStatus, LocalAccountInfo, PrivilegeControlStatus,
    RemoteAccessStatus, SecurityPostureV4, UpdateStatus, WeakProtocolsInfo,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AntiVirusProduct {
    display_name: Option<String>,
    product_state: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct QuickFix {
    installed_on: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EncryptableVolume {
    drive_letter: Option<String>,
    protection_status: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32UserAccount {
    name: Option<String>,
    full_name: Option<String>,
    disabled: Option<bool>,
    password_required: Option<bool>,
}

fn hklm(path: &str) -> Option<RegKey> {
    RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(path).ok()
}

fn reg_dword(path: &str, name: &str) -> Option<u32> {
    hklm(path).and_then(|k| k.get_value::<u32, _>(name).ok())
}

pub fn collect() -> SecurityPostureV4 {
    let av = antivirus();
    let (wu_status, last_update) = windows_update();
    let (bitlocker, volumes) = bitlocker();
    let rdp = reg_dword(
        r"SYSTEM\CurrentControlSet\Control\Terminal Server",
        "fDenyTSConnections",
    )
    .map(|v| v == 0);
    let ssh = ssh_enabled();
    let mut services = Vec::new();
    if rdp == Some(true) {
        services.push("rdp".to_string());
    }
    if ssh == Some(true) {
        services.push("ssh".to_string());
    }
    SecurityPostureV4 {
        // Legacy flat fields
        antivirus: av.clone(),
        windows_update_status: wu_status.clone(),
        bitlocker: bitlocker.clone(),
        rdp_enabled: rdp,
        firewall_enabled: firewall_enabled(),
        uac_enabled: uac_enabled(),
        secure_boot_enabled: secure_boot_enabled(),
        usb_storage_blocked: usb_storage_blocked(),
        weak_protocols: Some(weak_protocols()),
        listening_ports: Some(crate::ports::collect()),
        startup_programs: Some(crate::startup::collect()),
        local_accounts: Some(local_accounts()),
        // v4 objects
        endpoint_protection: av,
        update: UpdateStatus {
            status: wu_status.clone().unwrap_or_else(|| "unknown".into()),
            enabled: Some(wu_status.as_deref() != Some("disabled")),
            last_updated_at: last_update,
            reboot_required: Some(reboot_required()),
            ..Default::default()
        },
        disk_encryption: DiskEncryptionStatus {
            enabled: Some(bitlocker.as_deref() == Some("on")),
            technology: Some("bitlocker".into()),
            encrypted_volumes: volumes,
        },
        remote_access: RemoteAccessStatus {
            remote_desktop_enabled: rdp,
            ssh_enabled: ssh,
            services: (!services.is_empty()).then_some(services),
        },
        privilege_control: PrivilegeControlStatus {
            sudo_installed: Some(sudo_installed()),
            root_account_locked: None,
        },
        ..Default::default()
    }
}

/// AntivirusInfo: displayName+name=tên; status="enabled"/"disabled";
/// enabled = bit 0x1000; up_to_date = bit 0x0010 KHÔNG set.
fn antivirus() -> Option<Vec<AntivirusInfo>> {
    let c = wmi::conn_ns("root\\SecurityCenter2")?;
    let rows: Vec<AntiVirusProduct> =
        wmi::query(&c, "SELECT displayName, productState FROM AntiVirusProduct");
    let list: Vec<AntivirusInfo> = rows
        .iter()
        .filter_map(|r| {
            let name = r.display_name.clone()?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let st = r.product_state.unwrap_or(0);
            let enabled = st & 0x1000 != 0;
            Some(AntivirusInfo {
                display_name: Some(name.clone()),
                name: Some(name),
                status: Some(if enabled { "enabled" } else { "disabled" }.into()),
                enabled,
                up_to_date: st & 0x0010 == 0,
            })
        })
        .collect();
    (!list.is_empty()).then_some(list)
}

/// GetWindowsUpdateInfo → (status "up-to-date"|"outdated"|"unknown", lastUpdateDate ISO).
fn windows_update() -> (Option<String>, Option<String>) {
    let mut latest = latest_qfe_date();
    if latest.is_none() {
        latest = hklm(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\Results\Install",
        )
        .and_then(|k| k.get_value::<String, _>("LastSuccessTime").ok())
        .and_then(|s| {
            chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|n| n.and_utc())
        });
    }
    match latest {
        Some(d) => {
            let age = chrono::Utc::now().signed_duration_since(d).num_days();
            (
                Some(if age <= 45 { "up-to-date" } else { "outdated" }.into()),
                Some(d.to_rfc3339()),
            )
        }
        None => (Some("unknown".into()), None),
    }
}

fn latest_qfe_date() -> Option<chrono::DateTime<chrono::Utc>> {
    let c = wmi::conn()?;
    let rows: Vec<QuickFix> = wmi::query(&c, "SELECT InstalledOn FROM Win32_QuickFixEngineering");
    rows.iter()
        .filter_map(|r| parse_qfe_date(r.installed_on.as_deref()))
        .max()
}

/// QFE InstalledOn: "M/D/YYYY" hoặc "DD/MM/YYYY".
fn parse_qfe_date(s: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s?.trim();
    for fmt in ["%m/%d/%Y", "%d/%m/%Y", "%Y-%m-%d"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return d.and_hms_opt(0, 0, 0).map(|n| n.and_utc());
        }
    }
    None
}

/// (status, encrypted_volumes): "on" khi C: hoặc bất kỳ volume nào ProtectionStatus==1.
fn bitlocker() -> (Option<String>, Option<Vec<String>>) {
    let Some(c) = wmi::conn_ns("root\\CIMV2\\Security\\MicrosoftVolumeEncryption") else {
        return (None, None);
    };
    let rows: Vec<EncryptableVolume> = wmi::query(
        &c,
        "SELECT DriveLetter, ProtectionStatus FROM Win32_EncryptableVolume",
    );
    let volumes: Vec<String> = rows
        .iter()
        .filter(|r| r.protection_status == Some(1))
        .filter_map(|r| r.drive_letter.clone())
        .collect();
    let status = if volumes.is_empty() { "off" } else { "on" };
    (
        Some(status.into()),
        (!volumes.is_empty()).then_some(volumes),
    )
}

fn reboot_required() -> bool {
    hklm(r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired")
        .is_some()
        || hklm(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
        )
        .is_some()
}

fn ssh_enabled() -> Option<bool> {
    hklm(r"SYSTEM\CurrentControlSet\Services\sshd")
        .and_then(|k| k.get_value::<u32, _>("Start").ok())
        .map(|s| s == 2 || s == 3)
}

fn sudo_installed() -> bool {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    std::path::Path::new(&root)
        .join(r"System32\sudo.exe")
        .exists()
}

fn firewall_enabled() -> Option<bool> {
    reg_dword(
        r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters\FirewallPolicy\StandardProfile",
        "EnableFirewall",
    )
    .map(|v| v == 1)
}

fn uac_enabled() -> Option<bool> {
    reg_dword(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System",
        "EnableLUA",
    )
    .map(|v| v == 1)
}

fn secure_boot_enabled() -> Option<bool> {
    reg_dword(
        r"SYSTEM\CurrentControlSet\Control\SecureBoot\State",
        "UEFISecureBootEnabled",
    )
    .map(|v| v == 1)
}

fn usb_storage_blocked() -> Option<bool> {
    reg_dword(r"SYSTEM\CurrentControlSet\Services\USBSTOR", "Start").map(|v| v == 4)
}

/// WeakProtocolsInfo mang semantics *_disabled (default true = đã tắt).
fn weak_protocols() -> WeakProtocolsInfo {
    let mut w = WeakProtocolsInfo::default();
    if reg_dword(
        r"SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters",
        "SMB1",
    ) == Some(1)
    {
        w.smbv1_disabled = false;
    }
    for (proto, flag) in [("TLS 1.0", 0), ("TLS 1.1", 1), ("SSL 3.0", 2)] {
        let enabled = reg_dword(
            &format!(
                r"SYSTEM\CurrentControlSet\Control\SecurityProviders\SCHANNEL\Protocols\{proto}\Server"
            ),
            "Enabled",
        ) == Some(1);
        if enabled {
            match flag {
                0 => w.tls10_disabled = false,
                1 => w.tls11_disabled = false,
                _ => w.ssl3_disabled = false,
            }
        }
    }
    w
}

fn local_accounts() -> Vec<LocalAccountInfo> {
    let Some(c) = wmi::conn() else {
        return Vec::new();
    };
    let rows: Vec<Win32UserAccount> = wmi::query(
        &c,
        "SELECT Name, FullName, Disabled, PasswordRequired FROM Win32_UserAccount WHERE LocalAccount = TRUE",
    );
    let mut seen = BTreeMap::new();
    for r in rows {
        let Some(name) = r
            .name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        seen.entry(name.to_lowercase())
            .or_insert_with(|| LocalAccountInfo {
                username: Some(name.clone()),
                name: Some(name.clone()),
                full_name: r.full_name,
                disabled: r.disabled.unwrap_or(false),
                has_password: Some(r.password_required.unwrap_or(false)),
                is_admin: name.eq_ignore_ascii_case("Administrator"),
            });
    }
    seen.into_values().collect()
}
