//! Port of MacosOsCollector.cs.

use crate::cmd;
use agent_core::schema::{BiosInfo, CpuInfo, DiskInfo, MainboardInfo};
use serde_json::Value;
use std::sync::OnceLock;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SwVers {
    pub name: Option<String>,
    pub version: Option<String>,
    pub build: Option<String>,
}

pub fn sw_vers() -> SwVers {
    parse_sw_vers(cmd::run("/usr/bin/sw_vers", &[], 10_000).as_deref())
}

pub fn parse_sw_vers(output: Option<&str>) -> SwVers {
    let mut r = SwVers::default();
    for line in output.unwrap_or_default().lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let v = Some(v.trim().to_string());
        match k.trim() {
            "ProductName" => r.name = v,
            "ProductVersion" => r.version = v,
            "BuildVersion" => r.build = v,
            _ => {}
        }
    }
    r
}

pub fn os_pretty_name() -> Option<String> {
    let v = sw_vers();
    let name = v.name?;
    Some(match v.version {
        Some(ver) => format!("{name} {ver}"),
        None => name,
    })
}

pub fn cpu() -> Option<CpuInfo> {
    // machdep.cpu.brand_string is Intel-only; Apple Silicon → chip_type, then hw.model.
    let model = cmd::sysctl("machdep.cpu.brand_string")
        .filter(|s| !s.is_empty())
        .or_else(|| hardware_field(&["chip_type"]))
        .or_else(|| cmd::sysctl("hw.model").filter(|s| !s.is_empty()))?;
    Some(CpuInfo {
        model: Some(model),
        cores: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as i32),
    })
}

pub fn ram_gb() -> Option<f64> {
    let bytes: i64 = cmd::sysctl("hw.memsize")?.parse().ok()?;
    Some((bytes as f64 / GIB * 10.0).round() / 10.0)
}

pub fn disks() -> Option<Vec<DiskInfo>> {
    parse_storage_data_type(&cmd::system_profiler_json("SPStorageDataType", 30_000)?)
}

/// Volumes grouped by physical drive model; size = largest volume on that drive.
pub fn parse_storage_data_type(root: &Value) -> Option<Vec<DiskInfo>> {
    let items = root.get("SPStorageDataType")?.as_array()?;
    let mut disks: Vec<DiskInfo> = Vec::new();
    for vol in items {
        let Some(drive) = vol.get("physical_drive").filter(|d| d.is_object()) else {
            continue;
        };
        let Some(model) = cmd::json_string(drive, &["device_name", "media_name"]) else {
            continue;
        };
        let medium = cmd::json_string(drive, &["medium_type"])
            .unwrap_or_default()
            .to_lowercase();
        let size = vol
            .get("size_in_bytes")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if let Some(existing) = disks.iter_mut().find(|d| {
            d.model
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case(&model))
        }) {
            if size > existing.size_bytes.unwrap_or(0) {
                existing.size_bytes = Some(size);
            }
            continue;
        }
        disks.push(DiskInfo {
            model: Some(model),
            serial: None,
            size_bytes: (size > 0).then_some(size),
            size_gb: (size > 0).then(|| (size as f64 / GIB).round()),
            r#type: if medium.contains("ssd") {
                Some("SSD".into())
            } else if medium.contains("hdd") || medium.contains("rotational") {
                Some("HDD".into())
            } else {
                None
            },
        });
    }
    (!disks.is_empty()).then_some(disks)
}

/// SPHardwareDataType is static for the process lifetime and slow (~1s) → cache it.
fn hardware() -> Option<&'static Value> {
    static HW: OnceLock<Option<Value>> = OnceLock::new();
    HW.get_or_init(|| cmd::system_profiler_json("SPHardwareDataType", 30_000))
        .as_ref()
}

pub fn hardware_field(keys: &[&str]) -> Option<String> {
    cmd::json_string(cmd::first_item(hardware()?, "SPHardwareDataType")?, keys)
}

pub fn mainboard() -> Option<MainboardInfo> {
    parse_mainboard(hardware()?)
}

pub fn parse_mainboard(root: &Value) -> Option<MainboardInfo> {
    let hw = cmd::first_item(root, "SPHardwareDataType")?;
    let id = cmd::json_string(hw, &["machine_model", "model_identifier"]);
    let model = match cmd::json_string(hw, &["machine_name", "model_name"]) {
        Some(name) => Some(match id {
            Some(id) => format!("{name} ({id})"),
            None => name,
        }),
        None => id,
    };
    let serial = cmd::json_string(hw, &["serial_number"]);
    if model.is_none() && serial.is_none() {
        return None;
    }
    Some(MainboardInfo { model, serial })
}

pub fn bios() -> Option<BiosInfo> {
    let v =
        hardware_field(&["boot_rom_version"]).or_else(|| hardware_field(&["os_loader_version"]))?;
    Some(BiosInfo { version: Some(v) })
}

pub fn gpu_model() -> Option<String> {
    parse_gpu_model(&cmd::system_profiler_json("SPDisplaysDataType", 30_000)?)
}

/// Intel: spdisplays_ndrvs[] per GPU; Apple Silicon: first entry's sppci_model = chip.
pub fn parse_gpu_model(root: &Value) -> Option<String> {
    let first = cmd::first_item(root, "SPDisplaysDataType")?;
    if let Some(gpu) = first
        .get("spdisplays_ndrvs")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    {
        return cmd::json_string(gpu, &["_name", "sppci_model"]);
    }
    cmd::json_string(first, &["_name", "sppci_model", "spdisplays_gpu"])
}

pub fn is_vm() -> Option<bool> {
    is_vm_from(
        cmd::sysctl("kern.hv_vmm_present").as_deref(),
        cmd::sysctl("hw.model").as_deref(),
    )
}

pub fn is_vm_from(hv_vmm_present: Option<&str>, hw_model: Option<&str>) -> Option<bool> {
    if hv_vmm_present == Some("1") {
        return Some(true);
    }
    let model = hw_model.unwrap_or_default().to_lowercase();
    if model.is_empty() {
        return None;
    }
    Some(
        [
            "vmware",
            "virtualbox",
            "parallels",
            "qemu",
            "virtualmac",
            "vm",
        ]
        .iter()
        .any(|m| model.contains(m)),
    )
}

pub fn logged_user() -> String {
    console_user(cmd::run("/usr/bin/stat", &["-f", "%Su", "/dev/console"], 10_000).as_deref())
        .unwrap_or_else(agent_core::current_user)
}

pub fn console_user(stat_output: Option<&str>) -> Option<String> {
    let u = stat_output?.trim();
    (!u.is_empty() && u != "root" && !u.starts_with('_')).then(|| u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sw_vers_extracts_name_version_build() {
        let r = parse_sw_vers(Some(
            "ProductName:\tmacOS\nProductVersion:\t14.5\nBuildVersion:\t23F79\n",
        ));
        assert_eq!(r.name.as_deref(), Some("macOS"));
        assert_eq!(r.version.as_deref(), Some("14.5"));
        assert_eq!(r.build.as_deref(), Some("23F79"));
    }

    #[test]
    fn sw_vers_none_input() {
        assert_eq!(parse_sw_vers(None), SwVers::default());
    }

    #[test]
    fn storage_groups_by_physical_drive() {
        let v = json!({"SPStorageDataType":[
          {"_name":"Macintosh HD","bsd_name":"disk3s1s1","file_system":"apfs","size_in_bytes":994662584320u64,
           "physical_drive":{"device_name":"APPLE SSD AP1024Z","is_internal_disk":"yes","medium_type":"ssd"}},
          {"_name":"Data","bsd_name":"disk3s5","file_system":"apfs","size_in_bytes":994662584320u64,
           "physical_drive":{"device_name":"APPLE SSD AP1024Z","is_internal_disk":"yes","medium_type":"ssd"}},
          {"_name":"NoDriveVol","bsd_name":"disk9s1"}
        ]});
        let d = parse_storage_data_type(&v).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].model.as_deref(), Some("APPLE SSD AP1024Z"));
        assert_eq!(d[0].r#type.as_deref(), Some("SSD"));
        assert_eq!(d[0].size_bytes, Some(994662584320));
        assert_eq!(d[0].size_gb, Some(926.0));
    }

    #[test]
    fn storage_without_physical_drives_is_none() {
        assert_eq!(
            parse_storage_data_type(&json!({"SPStorageDataType":[{"_name":"vol"}]})),
            None
        );
    }

    #[test]
    fn mainboard_model_with_identifier() {
        let v = json!({"SPHardwareDataType":[{"machine_name":"MacBook Pro","machine_model":"Mac15,3","serial_number":"C02X"}]});
        let m = parse_mainboard(&v).unwrap();
        assert_eq!(m.model.as_deref(), Some("MacBook Pro (Mac15,3)"));
        assert_eq!(m.serial.as_deref(), Some("C02X"));
        assert_eq!(parse_mainboard(&json!({"SPHardwareDataType":[{}]})), None);
    }

    #[test]
    fn gpu_intel_and_apple_silicon() {
        let intel =
            json!({"SPDisplaysDataType":[{"spdisplays_ndrvs":[{"_name":"Intel Iris Plus"}]}]});
        assert_eq!(parse_gpu_model(&intel).as_deref(), Some("Intel Iris Plus"));
        let arm = json!({"SPDisplaysDataType":[{"_name":"Apple M3","sppci_model":"Apple M3"}]});
        assert_eq!(parse_gpu_model(&arm).as_deref(), Some("Apple M3"));
    }

    #[test]
    fn vm_detection() {
        assert_eq!(is_vm_from(Some("1"), Some("MacBookPro18,1")), Some(true));
        assert_eq!(is_vm_from(Some("0"), Some("VirtualMac2,1")), Some(true));
        assert_eq!(is_vm_from(Some("0"), Some("Mac15,3")), Some(false));
        assert_eq!(is_vm_from(None, None), None);
    }

    #[test]
    fn console_user_filters_system_accounts() {
        assert_eq!(console_user(Some("alice\n")).as_deref(), Some("alice"));
        assert_eq!(console_user(Some("root")), None);
        assert_eq!(console_user(Some("_mbsetupuser")), None);
    }
}
