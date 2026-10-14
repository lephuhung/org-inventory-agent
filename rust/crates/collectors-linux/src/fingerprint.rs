//! Port of LinuxFingerprintCollector.cs: product_uuid (raw), machine-id and board_serial (SHA-256 hex).

use agent_core::schema::FingerprintPayload;
use sha2::{Digest, Sha256};

pub fn collect() -> FingerprintPayload {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    from_raw(
        read("/sys/class/dmi/id/product_uuid").as_deref(),
        read("/etc/machine-id").as_deref(),
        read("/sys/class/dmi/id/board_serial").as_deref(),
    )
}

pub fn from_raw(
    uuid: Option<&str>,
    machine_id: Option<&str>,
    board_serial: Option<&str>,
) -> FingerprintPayload {
    FingerprintPayload {
        smbios_uuid: sanitize(uuid),
        machine_guid: hash_or_none(machine_id),
        mainboard_serial: hash_or_none(board_serial),
    }
}

pub fn sanitize(v: Option<&str>) -> Option<String> {
    let v = v?.trim();
    if v.is_empty() {
        return None;
    }
    const JUNK: [&str; 9] = [
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
    (!JUNK.contains(&v.to_lowercase().as_str())).then(|| v.to_string())
}

fn hash_or_none(raw: Option<&str>) -> Option<String> {
    sanitize(raw).map(|c| hex::encode(Sha256::digest(c.as_bytes())))
}
