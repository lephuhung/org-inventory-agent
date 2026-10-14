//! Port of FingerprintCollector.cs — fingerprint cố định theo machine:
//! - `smbios_uuid`   — Win32_ComputerSystemProduct.UUID (raw, sanitized)
//! - `machine_guid`  — HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid (SHA-256 hex)
//! - `mainboard_serial` — Win32_BaseBoard.SerialNumber (SHA-256 hex)

use crate::wmi;
use agent_core::schema::FingerprintPayload;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32ComputerSystemProduct {
    #[serde(rename = "UUID")]
    uuid: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32BaseBoard {
    serial_number: Option<String>,
}

const SANITIZE_LIST: &[&str] = &[
    "none",
    "default string",
    "to be filled by o.e.m.",
    "not applicable",
    "system serial number",
    "unknown",
    "n/a",
    "00000000-0000-0000-0000-000000000000",
    "00000000-0000-0000-0000-000000000001",
];

fn sanitize(v: Option<&str>) -> Option<String> {
    let v = v?.trim();
    if v.is_empty() || SANITIZE_LIST.contains(&v.to_lowercase().as_str()) {
        None
    } else {
        Some(v.to_string())
    }
}

pub fn sha256_hex(input: &str) -> String {
    let h = Sha256::digest(input.as_bytes());
    hex::encode(h)
}

pub fn collect() -> FingerprintPayload {
    let mut fp = FingerprintPayload::default();
    let Some(c) = wmi::conn() else {
        fp.machine_guid = read_machine_guid();
        return fp;
    };
    let rows: Vec<Win32ComputerSystemProduct> =
        wmi::query(&c, "SELECT UUID FROM Win32_ComputerSystemProduct");
    fp.smbios_uuid = rows.first().and_then(|r| sanitize(r.uuid.as_deref()));
    let boards: Vec<Win32BaseBoard> = wmi::query(&c, "SELECT SerialNumber FROM Win32_BaseBoard");
    fp.mainboard_serial = boards
        .first()
        .and_then(|b| sanitize(b.serial_number.as_deref()))
        .map(|s| sha256_hex(&s));
    fp.machine_guid = read_machine_guid();
    fp
}

fn read_machine_guid() -> Option<String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let raw = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Cryptography")
        .and_then(|k| k.get_value::<String, _>("MachineGuid"))
        .ok()?;
    sanitize(Some(&raw)).map(|g| sha256_hex(&g))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_placeholders() {
        assert!(sanitize(Some("To be filled by O.E.M.")).is_none());
        assert!(sanitize(Some("  ")).is_none());
        assert!(sanitize(Some("00000000-0000-0000-0000-000000000000")).is_none());
        assert_eq!(sanitize(Some(" abc ")).as_deref(), Some("abc"));
    }

    #[test]
    fn sha256_hex_lowercase() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
