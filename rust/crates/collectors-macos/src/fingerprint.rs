//! Port of MacosFingerprintCollector.cs:
//! smbios_uuid ← IOPlatformUUID (raw), machine_guid ← SHA-256(kern.hostuuid),
//! mainboard_serial ← SHA-256(hardware serial).

use crate::{cmd, os};
use agent_core::schema::FingerprintPayload;
use sha2::{Digest, Sha256};

pub fn collect() -> FingerprintPayload {
    let raw_uuid = io_platform_uuid().or_else(|| os::hardware_field(&["platform_UUID"]));
    // kern.hostuuid is missing on Apple Virtualization VMs → hash platform_UUID instead.
    let raw_host_uuid = cmd::sysctl("kern.hostuuid")
        .filter(|s| !s.is_empty())
        .or_else(|| os::hardware_field(&["platform_UUID"]));
    let raw_serial = os::hardware_field(&["serial_number"]);
    log::debug!(
        "macOS fingerprint: ioplatform_uuid={}, hostuuid={}, serial={}",
        if raw_uuid.is_some() { "set" } else { "(null)" },
        if raw_host_uuid.is_some() {
            "set"
        } else {
            "(null)"
        },
        if raw_serial.is_some() {
            "set"
        } else {
            "(null)"
        },
    );
    FingerprintPayload {
        smbios_uuid: sanitize(raw_uuid.as_deref()),
        machine_guid: hash_or_none(raw_host_uuid.as_deref()),
        mainboard_serial: hash_or_none(raw_serial.as_deref()),
    }
}

pub fn io_platform_uuid() -> Option<String> {
    parse_io_platform_uuid(
        cmd::run(
            "/usr/sbin/ioreg",
            &["-rd1", "-c", "IOPlatformExpertDevice"],
            10_000,
        )
        .as_deref(),
    )
}

/// `"IOPlatformUUID" = "XXXX-..."` → first quoted value after `=`.
pub fn parse_io_platform_uuid(output: Option<&str>) -> Option<String> {
    let output = output?;
    let idx = output.find("\"IOPlatformUUID\"")?;
    let eq = idx + output[idx..].find('=')?;
    let rest = output[eq + 1..].trim();
    let first = rest.find('"')?;
    let close = first + 1 + rest[first + 1..].find('"')?;
    Some(rest[first + 1..close].to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_io_platform_uuid_extracts_quoted_uuid() {
        let ioreg = r#"+-o IOPlatformExpertDevice  <class IOPlatformExpertDevice>
    {
      "IOPlatformUUID" = "A1B2C3D4-1234-5678-9ABC-DEF012345678"
      "serial-number" = <"C02ABCDEFGH">
    }"#;
        assert_eq!(
            parse_io_platform_uuid(Some(ioreg)).as_deref(),
            Some("A1B2C3D4-1234-5678-9ABC-DEF012345678")
        );
    }

    #[test]
    fn parse_io_platform_uuid_missing_key() {
        assert_eq!(parse_io_platform_uuid(Some("+-o Root")), None);
        assert_eq!(parse_io_platform_uuid(None), None);
    }

    #[test]
    fn hashes_match_dotnet_lower_hex() {
        assert_eq!(
            hash_or_none(Some(" abc ")).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(hash_or_none(Some("Unknown")), None);
    }
}
