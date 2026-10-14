//! WindowsInventoryProvider — envelope (agent/os/security v4) + snapshot flat.

use agent_core::payload::InventoryProvider;
use agent_core::schema::{AgentMetadata, InventoryEnvelope, InventorySnapshot};

pub struct WindowsInventoryProvider;

pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        "arm" => "arm",
        other => other,
    }
}

impl InventoryProvider for WindowsInventoryProvider {
    fn envelope(&self) -> InventoryEnvelope {
        InventoryEnvelope {
            inventory_schema_version: 4,
            agent: AgentMetadata {
                name: agent_core::APP_NAME.into(),
                version: agent_core::VERSION.into(),
                runtime: "Rust".into(),
                platform: "windows".into(),
                architecture: arch().into(),
                package_type: Some("msi".into()),
            },
            os: crate::os::os_metadata(),
            security: crate::security::collect(),
        }
    }

    fn snapshot(&self) -> InventorySnapshot {
        let c = crate::wmi::conn();
        InventorySnapshot {
            os_name: c.as_ref().and_then(|c| crate::os::os_name(c)),
            os_version: crate::os::os_version(),
            os_build: crate::os::os_build(),
            os_arch: Some(arch().into()),
            os_installed_at: crate::os::os_installed_at(),
            activation_status: c.as_ref().and_then(|c| crate::os::activation_status(c)),
            cpu: c.as_ref().and_then(|c| crate::os::cpu(c)),
            ram_gb: c.as_ref().and_then(|c| crate::os::ram_gb(c)),
            disks: c.as_ref().and_then(|c| crate::os::disks(c)),
            gpu: c.as_ref().and_then(|c| crate::os::gpu(c)),
            mainboard: c.as_ref().and_then(|c| crate::os::mainboard(c)),
            bios: c.as_ref().and_then(|c| crate::os::bios(c)),
            network: {
                let v = crate::network::collect();
                (!v.is_empty()).then_some(v)
            },
            logged_user: c.as_ref().and_then(|c| crate::os::logged_user(c)),
            installed_software: Some(crate::software::collect()),
            is_vm: c.as_ref().and_then(|c| crate::os::is_vm(c)),
        }
    }
}
