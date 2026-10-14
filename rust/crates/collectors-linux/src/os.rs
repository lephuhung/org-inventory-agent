//! Port of LinuxOsCollector.cs: CPU/RAM/disks/mainboard/BIOS/GPU/VM detection from /proc and /sys.

use agent_core::schema::{BiosInfo, CpuInfo, DiskInfo, MainboardInfo};
use std::fs;
use std::path::Path;

pub(crate) fn safe_read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

pub fn os_release_value(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    content
        .lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .map(|v| v.trim_matches('"').to_string())
}

pub fn cpu() -> CpuInfo {
    CpuInfo {
        model: safe_read("/proc/cpuinfo").and_then(|c| parse_cpu_model(&c)),
        cores: std::thread::available_parallelism()
            .ok()
            .map(|n| n.get() as i32),
    }
}

pub fn parse_cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
}

pub fn ram_gb() -> Option<f64> {
    parse_mem_total_gb(&fs::read_to_string("/proc/meminfo").ok()?)
}

pub fn parse_mem_total_gb(meminfo: &str) -> Option<f64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some((kb / (1024.0 * 1024.0) * 10.0).round() / 10.0)
}

pub fn disks() -> Option<Vec<DiskInfo>> {
    disks_from(Path::new("/sys/block"))
}

pub fn disks_from(sys_block: &Path) -> Option<Vec<DiskInfo>> {
    let mut entries: Vec<_> = fs::read_dir(sys_block).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("fd") {
            continue;
        }
        let dir = e.path();
        let model = safe_read(dir.join("device/model")).unwrap_or_else(|| name.clone());
        let Some(sectors) = safe_read(dir.join("size")).and_then(|s| s.parse::<i64>().ok()) else {
            continue;
        };
        if sectors == 0 {
            continue;
        }
        let bytes = sectors * 512;
        let lower = model.to_lowercase();
        let kind = if lower.contains("nvme") || lower.contains("ssd") {
            "SSD"
        } else {
            "HDD"
        };
        out.push(DiskInfo {
            model: Some(model),
            serial: safe_read(dir.join("device/serial")),
            size_bytes: Some(bytes),
            size_gb: Some((bytes as f64 / (1024.0 * 1024.0 * 1024.0)).round()),
            r#type: Some(kind.to_string()),
        });
    }
    (!out.is_empty()).then_some(out)
}

pub fn mainboard() -> Option<MainboardInfo> {
    let vendor = safe_read("/sys/class/dmi/id/board_vendor");
    let name = safe_read("/sys/class/dmi/id/board_name");
    let model = [vendor, name]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let serial = safe_read("/sys/class/dmi/id/board_serial");
    if model.trim().is_empty() && serial.as_deref().is_none_or(|s| s.trim().is_empty()) {
        return None;
    }
    Some(MainboardInfo {
        model: (!model.trim().is_empty()).then_some(model),
        serial,
    })
}

pub fn bios() -> Option<BiosInfo> {
    safe_read("/sys/class/dmi/id/bios_version").map(|v| BiosInfo { version: Some(v) })
}

pub fn gpu_model() -> Option<String> {
    let mut entries: Vec<_> = fs::read_dir("/sys/class/drm").ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .filter(|e| e.file_name().to_string_lossy().starts_with("card"))
        .find_map(|e| safe_read(e.path().join("device/label")))
}

pub fn is_vm() -> Option<bool> {
    let vendor = safe_read("/sys/class/dmi/id/sys_vendor").unwrap_or_default();
    let product = safe_read("/sys/class/dmi/id/product_name").unwrap_or_default();
    is_vm_from(&vendor, &product)
}

pub fn is_vm_from(vendor: &str, product: &str) -> Option<bool> {
    const MARKERS: [&str; 8] = [
        "vmware",
        "virtualbox",
        "qemu",
        "kvm",
        "hyper-v",
        "virtual machine",
        "xen",
        "innotek",
    ];
    let haystack = format!("{vendor} {product}").to_lowercase();
    if haystack.trim().is_empty() {
        return None;
    }
    Some(MARKERS.iter().any(|m| haystack.contains(m)))
}
