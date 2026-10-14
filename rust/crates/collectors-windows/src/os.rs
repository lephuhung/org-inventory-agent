//! Port of InventoryCollector.cs (Windows): os_name/version/build/installed_at/activation
//! + CPU/RAM/disks/GPU/mainboard/BIOS/is_vm/logged_user.

use crate::wmi;
use agent_core::schema::{BiosInfo, CpuInfo, DiskInfo, GpuInfo, MainboardInfo, OsMetadata};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32OperatingSystem {
    caption: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32Processor {
    name: Option<String>,
    number_of_cores: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32ComputerSystem {
    total_physical_memory: Option<u64>,
    user_name: Option<String>,
    manufacturer: Option<String>,
    model: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32DiskDrive {
    model: Option<String>,
    serial_number: Option<String>,
    size: Option<u64>,
    media_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32VideoController {
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32BaseBoard {
    manufacturer: Option<String>,
    product: Option<String>,
    serial_number: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Win32Bios {
    #[serde(rename = "SMBIOSBIOSVersion")]
    smbios_bios_version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SoftwareLicensingProduct {
    license_status: Option<String>,
}

const CV_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

fn cv_key() -> Option<winreg::RegKey> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(CV_KEY)
        .ok()
}

fn reg_str(k: &winreg::RegKey, name: &str) -> Option<String> {
    k.get_value::<String, _>(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn reg_u32(k: &winreg::RegKey, name: &str) -> Option<u32> {
    k.get_value::<u32, _>(name).ok()
}

/// GetDisplayVersion: DisplayVersion → ReleaseId → OSVersion.
fn display_version() -> Option<String> {
    let k = cv_key()?;
    reg_str(&k, "DisplayVersion").or_else(|| reg_str(&k, "ReleaseId"))
}

/// GetOsName: WMI Caption (bỏ "Microsoft ") + DisplayVersion;
/// fallback registry ProductName (fix Win10→11 build ≥ 22000) + DisplayVersion.
pub fn os_name(c: &wmi::WMIConnection) -> Option<String> {
    let caption: Option<String> =
        wmi::query::<Win32OperatingSystem>(c, "SELECT Caption FROM Win32_OperatingSystem")
            .into_iter()
            .next()
            .and_then(|r| r.caption)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    let k = cv_key();
    let display = k.as_ref().and_then(|k| reg_str(k, "DisplayVersion"));
    if let Some(cap) = caption {
        let clean = cap.replace("Microsoft ", "").trim().to_string();
        return Some(match display {
            Some(d) => format!("{clean} {d}"),
            None => clean,
        });
    }
    if let Some(k) = &k {
        let mut product = reg_str(k, "ProductName");
        let build = reg_str(k, "CurrentBuildNumber");
        if let (Some(b), Some(p)) = (&build, &mut product) {
            if b.parse::<u32>().unwrap_or(0) >= 22000 && p.contains("Windows 10") {
                *p = p.replace("Windows 10", "Windows 11");
            }
        }
        if let Some(p) = product {
            return Some(match display {
                Some(d) => format!("{p} {d}"),
                None => p,
            });
        }
    }
    None
}

/// GetOsVersion: `{CurrentVersion}.{CurrentBuildNumber}`.
pub fn os_version() -> Option<String> {
    let k = cv_key()?;
    match (
        reg_str(&k, "CurrentVersion"),
        reg_str(&k, "CurrentBuildNumber"),
    ) {
        (Some(v), Some(b)) => Some(format!("{v}.{b}")),
        _ => None,
    }
}

pub fn os_build() -> Option<String> {
    cv_key().and_then(|k| reg_str(&k, "CurrentBuildNumber"))
}

/// GetOsInstalledAt: InstallDate unix → "yyyy-MM-ddTHH:mm:ssZ".
pub fn os_installed_at() -> Option<String> {
    let k = cv_key()?;
    let secs = reg_u32(&k, "InstallDate")
        .map(|v| v as i64)
        .or_else(|| k.get_value::<u64, _>("InstallDate").ok().map(|v| v as i64))?;
    chrono::DateTime::from_timestamp(secs, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// GetActivationStatus: hàng đầu tiên có PartialProductKey; "1"→licensed else unlicensed.
pub fn activation_status(c: &wmi::WMIConnection) -> Option<String> {
    let rows: Vec<SoftwareLicensingProduct> = wmi::query(
        c,
        "SELECT LicenseStatus FROM SoftwareLicensingProduct WHERE PartialProductKey IS NOT NULL",
    );
    rows.into_iter().next().and_then(|r| {
        r.license_status.map(|s| {
            if s.trim() == "1" {
                "licensed".to_string()
            } else {
                "unlicensed".to_string()
            }
        })
    })
}

/// Envelope OsMetadata: distribution="windows", version=DisplayVersion, kernel=OSVersion.
pub fn os_metadata() -> OsMetadata {
    OsMetadata {
        platform: "windows".into(),
        distribution: Some("windows".into()),
        distribution_version: display_version(),
        kernel_version: kernel_version(),
        architecture: Some(crate::provider::arch().into()),
        subscription: None,
    }
}

/// Environment.OSVersion.Version — "10.0.22631.0".
fn kernel_version() -> Option<String> {
    let k = cv_key()?;
    match (
        reg_str(&k, "CurrentVersion"),
        reg_str(&k, "CurrentBuildNumber"),
        reg_u32(&k, "UBR"),
    ) {
        (Some(v), Some(b), ubr) => Some(format!("{v}.{b}.{}", ubr.unwrap_or(0))),
        _ => None,
    }
}

/// GetCpu: processor đầu tiên — {model, cores}.
pub fn cpu(c: &wmi::WMIConnection) -> Option<CpuInfo> {
    let rows: Vec<Win32Processor> =
        wmi::query(c, "SELECT Name, NumberOfCores FROM Win32_Processor");
    rows.into_iter().next().and_then(|p| {
        let model = p.name.map(|s| s.trim().to_string());
        if model.is_none() && p.number_of_cores.is_none() {
            return None;
        }
        Some(CpuInfo {
            model,
            cores: p.number_of_cores,
        })
    })
}

/// GetRamGb: TotalPhysicalMemory / 2^30, round 1 số.
pub fn ram_gb(c: &wmi::WMIConnection) -> Option<f64> {
    let rows: Vec<Win32ComputerSystem> =
        wmi::query(c, "SELECT TotalPhysicalMemory FROM Win32_ComputerSystem");
    rows.into_iter()
        .next()
        .and_then(|r| r.total_physical_memory)
        .map(|b| (b as f64 / 1073741824.0 * 10.0).round() / 10.0)
}

/// GetDisks: bỏ size==0; InferDiskType(mediaType, model).
pub fn disks(c: &wmi::WMIConnection) -> Option<Vec<DiskInfo>> {
    let rows: Vec<Win32DiskDrive> = wmi::query(
        c,
        "SELECT Model, SerialNumber, Size, MediaType FROM Win32_DiskDrive",
    );
    let out: Vec<DiskInfo> = rows
        .into_iter()
        .filter(|d| d.size.unwrap_or(0) > 0)
        .map(|d| {
            let size = d.size.unwrap();
            let model = d.model.map(|s| s.trim().to_string());
            let r#type = infer_disk_type(d.media_type.as_deref(), model.as_deref());
            DiskInfo {
                model,
                serial: d.serial_number.map(|s| s.trim().to_string()),
                size_bytes: Some(size as i64),
                size_gb: Some((size as f64 / 1073741824.0).round()),
                r#type: Some(r#type),
            }
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn infer_disk_type(media: Option<&str>, model: Option<&str>) -> String {
    let m = media.unwrap_or("").to_lowercase();
    let md = model.unwrap_or("").to_lowercase();
    if m.contains("ssd") || m.contains("solid") || md.contains("nvme") || md.contains("ssd") {
        if md.contains("nvme") {
            "NVMe".into()
        } else {
            "SSD".into()
        }
    } else {
        "HDD".into()
    }
}

pub fn gpu(c: &wmi::WMIConnection) -> Option<GpuInfo> {
    let rows: Vec<Win32VideoController> = wmi::query(c, "SELECT Name FROM Win32_VideoController");
    rows.into_iter()
        .filter_map(|g| g.name.map(|s| s.trim().to_string()))
        .find(|s| !s.is_empty())
        .map(|m| GpuInfo { model: Some(m) })
}

/// Mainboard { model = "manu product", serial }.
pub fn mainboard(c: &wmi::WMIConnection) -> Option<MainboardInfo> {
    let rows: Vec<Win32BaseBoard> = wmi::query(
        c,
        "SELECT Manufacturer, Product, SerialNumber FROM Win32_BaseBoard",
    );
    rows.into_iter().next().map(|b| {
        let model = [b.manufacturer, b.product]
            .into_iter()
            .flatten()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        MainboardInfo {
            model: (!model.is_empty()).then_some(model),
            serial: b.serial_number.map(|s| s.trim().to_string()),
        }
    })
}

pub fn bios(c: &wmi::WMIConnection) -> Option<BiosInfo> {
    let rows: Vec<Win32Bios> = wmi::query(c, "SELECT SMBIOSBIOSVersion FROM Win32_BIOS");
    rows.into_iter()
        .next()
        .and_then(|b| b.smbios_bios_version)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .map(|v| BiosInfo { version: Some(v) })
}

/// is_vm: manufacturer+model chứa marker ảo hóa.
pub fn is_vm(c: &wmi::WMIConnection) -> Option<bool> {
    let rows: Vec<Win32ComputerSystem> =
        wmi::query(c, "SELECT Manufacturer, Model FROM Win32_ComputerSystem");
    rows.into_iter().next().map(|s| {
        let vendor = format!(
            "{} {}",
            s.manufacturer.unwrap_or_default(),
            s.model.unwrap_or_default()
        )
        .to_lowercase();
        [
            "vmware",
            "virtualbox",
            "qemu",
            "kvm",
            "hyper-v",
            "virtual machine",
            "xen",
            "innotek",
        ]
        .iter()
        .any(|m| vendor.contains(m))
    })
}

/// LoggedUser: Win32_ComputerSystem.UserName → Environment.UserName.
pub fn logged_user(c: &wmi::WMIConnection) -> Option<String> {
    let rows: Vec<Win32ComputerSystem> = wmi::query(c, "SELECT UserName FROM Win32_ComputerSystem");
    rows.into_iter()
        .next()
        .and_then(|r| r.user_name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| Some(agent_core::current_user()).filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_type_inference() {
        assert_eq!(infer_disk_type(None, Some("Samsung NVMe PM9A1")), "NVMe");
        assert_eq!(infer_disk_type(Some("SSD"), None), "SSD");
        assert_eq!(
            infer_disk_type(Some("Fixed hard disk"), Some("ST1000DM")),
            "HDD"
        );
    }
}
