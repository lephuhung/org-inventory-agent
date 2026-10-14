//! Port of LinuxInventoryProvider.cs.

use agent_core::payload::InventoryProvider;
use agent_core::schema::{
    AgentMetadata, GpuInfo, InventoryEnvelope, InventorySnapshot, OsMetadata,
};

pub struct LinuxInventoryProvider;

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        "arm" => "arm",
        other => other,
    }
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn detect_package_type() -> Option<String> {
    if std::path::Path::new("/etc/debian_version").exists() {
        return Some("deb".into());
    }
    if std::path::Path::new("/etc/redhat-release").exists() {
        return Some("rpm".into());
    }
    let c = std::fs::read_to_string("/etc/os-release")
        .ok()?
        .to_lowercase();
    if ["rhel", "rocky", "almalinux", "fedora", "centos"]
        .iter()
        .any(|k| c.contains(k))
    {
        return Some("rpm".into());
    }
    if c.contains("ubuntu") || c.contains("debian") {
        return Some("deb".into());
    }
    None
}

pub fn os_metadata_from(os_release: &str, kernel: Option<String>, arch: &str) -> OsMetadata {
    let mut m = OsMetadata {
        platform: "linux".into(),
        architecture: Some(arch.into()),
        kernel_version: kernel,
        ..Default::default()
    };
    for line in os_release.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim_matches('"').to_string();
        match k {
            "ID" => m.distribution = Some(v),
            "VERSION_ID" => m.distribution_version = Some(v),
            _ => {}
        }
    }
    m
}

fn os_metadata() -> OsMetadata {
    os_metadata_from(
        &std::fs::read_to_string("/etc/os-release").unwrap_or_default(),
        read_trim("/proc/sys/kernel/osrelease"),
        arch(),
    )
}

fn pretty_name() -> Option<String> {
    std::fs::read_to_string("/etc/os-release")
        .ok()?
        .lines()
        .find_map(|l| {
            l.strip_prefix("PRETTY_NAME=")
                .map(|v| v.trim_matches('"').to_string())
        })
}

impl InventoryProvider for LinuxInventoryProvider {
    fn envelope(&self) -> InventoryEnvelope {
        InventoryEnvelope {
            inventory_schema_version: 4,
            agent: AgentMetadata {
                name: agent_core::APP_NAME.into(),
                version: agent_core::VERSION.into(),
                runtime: "Rust".into(),
                platform: "linux".into(),
                architecture: arch().into(),
                package_type: detect_package_type(),
            },
            os: os_metadata(),
            security: crate::security::collect(),
        }
    }

    fn snapshot(&self) -> InventorySnapshot {
        let meta = os_metadata();
        InventorySnapshot {
            os_name: pretty_name(),
            os_version: meta.distribution_version,
            os_build: meta.kernel_version,
            os_arch: Some(arch().into()),
            cpu: Some(crate::os::cpu()),
            ram_gb: crate::os::ram_gb(),
            disks: crate::os::disks(),
            gpu: crate::os::gpu_model().map(|m| GpuInfo { model: Some(m) }),
            mainboard: crate::os::mainboard(),
            bios: crate::os::bios(),
            network: agent_core::posix::network::collect(),
            logged_user: Some(agent_core::current_user()),
            installed_software: Some(crate::software::collect()),
            is_vm: crate::os::is_vm(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_metadata_parse() {
        let m = os_metadata_from(
            "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"22.04\"\n",
            Some("6.1".into()),
            "x64",
        );
        assert_eq!(m.distribution.as_deref(), Some("ubuntu"));
        assert_eq!(m.distribution_version.as_deref(), Some("22.04"));
        assert_eq!(m.kernel_version.as_deref(), Some("6.1"));
    }
}
