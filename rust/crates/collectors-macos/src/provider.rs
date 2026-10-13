//! Port of MacosInventoryProvider.cs.

use crate::{cmd, os};
use agent_core::payload::InventoryProvider;
use agent_core::schema::{
    AgentMetadata, GpuInfo, InventoryEnvelope, InventorySnapshot, OsMetadata,
};

pub struct MacosInventoryProvider;

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

/// "pkg" when installed via the installer package (pkgutil receipt), else "bin".
pub fn detect_package_type() -> Option<String> {
    let receipt = cmd::run(
        "/usr/sbin/pkgutil",
        &["--pkg-info", "com.orginventory.agent"],
        10_000,
    );
    Some(if receipt.is_some() { "pkg" } else { "bin" }.into())
}

pub fn os_metadata() -> OsMetadata {
    let v = os::sw_vers();
    OsMetadata {
        platform: "macos".into(),
        architecture: Some(arch().into()),
        distribution: Some(v.name.unwrap_or_else(|| "macOS".into())),
        distribution_version: v.version,
        // Kernel = Darwin release (uname -r); sw_vers BuildVersion is the OS build.
        kernel_version: cmd::sysctl("kern.osrelease")
            .filter(|s| !s.is_empty())
            .or(v.build),
        ..Default::default()
    }
}

impl InventoryProvider for MacosInventoryProvider {
    fn envelope(&self) -> InventoryEnvelope {
        InventoryEnvelope {
            inventory_schema_version: 4,
            agent: AgentMetadata {
                name: agent_core::APP_NAME.into(),
                version: agent_core::VERSION.into(),
                runtime: "Rust".into(),
                platform: "macos".into(),
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
            os_name: os::os_pretty_name(),
            os_version: meta.distribution_version,
            os_build: meta.kernel_version,
            os_arch: Some(arch().into()),
            cpu: os::cpu(),
            ram_gb: os::ram_gb(),
            disks: os::disks(),
            gpu: os::gpu_model().map(|m| GpuInfo { model: Some(m) }),
            mainboard: os::mainboard(),
            bios: os::bios(),
            network: agent_core::posix::network::collect(),
            logged_user: Some(os::logged_user()),
            installed_software: Some(crate::software::collect()),
            is_vm: os::is_vm(),
            ..Default::default()
        }
    }
}
