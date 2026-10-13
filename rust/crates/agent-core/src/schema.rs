//! Wire DTOs, port of Collectors/Schema/*.cs. JSON names are the backend contract; `None` fields are omitted
//! (the .NET agent serialized with `WhenWritingNull`).

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct FingerprintPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smbios_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mainboard_serial: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentMetadata {
    pub name: String,
    pub version: String,
    pub runtime: String,
    pub platform: String,
    pub architecture: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OsMetadata {
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kernel_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SecurityPostureV4 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub antivirus: Option<Vec<AntivirusInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_update_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitlocker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rdp_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firewall_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uac_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure_boot_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_storage_blocked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak_protocols: Option<WeakProtocolsInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listening_ports: Option<Vec<ListeningPortInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_programs: Option<Vec<StartupProgramInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_accounts: Option<Vec<LocalAccountInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smarts: Option<Vec<serde_json::Value>>,
    pub update: UpdateStatus,
    pub disk_encryption: DiskEncryptionStatus,
    pub remote_access: RemoteAccessStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_protection: Option<Vec<AntivirusInfo>>,
    pub privilege_control: PrivilegeControlStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UpdateStatus {
    /// "up-to-date" | "updates-available" | "outdated" | "unknown"
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_pending_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reboot_required: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<String>,
}

impl Default for UpdateStatus {
    fn default() -> Self {
        Self::unknown()
    }
}

impl UpdateStatus {
    pub fn unknown() -> Self {
        Self {
            status: "unknown".into(),
            enabled: None,
            pending_count: None,
            security_pending_count: None,
            reboot_required: None,
            last_updated_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DiskEncryptionStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technology: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_volumes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct RemoteAccessStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_desktop_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct PrivilegeControlStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sudo_installed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_account_locked: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct AntivirusInfo {
    #[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub enabled: bool,
    #[serde(rename = "upToDate")]
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LocalAccountInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_password: Option<bool>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ListeningPortInfo {
    pub port: i32,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct StartupProgramInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WeakProtocolsInfo {
    pub smbv1_disabled: bool,
    pub tls10_disabled: bool,
    pub tls11_disabled: bool,
    pub ssl3_disabled: bool,
}

impl Default for WeakProtocolsInfo {
    fn default() -> Self {
        Self {
            smbv1_disabled: true,
            tls10_disabled: true,
            tls11_disabled: true,
            ssl3_disabled: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct CpuInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskInfo {
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size_bytes: Option<i64>,
    pub size_gb: Option<f64>,
    /// SSD | HDD | NVMe
    pub r#type: Option<String>,
}

impl Serialize for DiskInfo {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(None)?;
        if let Some(v) = &self.model {
            m.serialize_entry("model", v)?;
        }
        if let Some(v) = &self.serial {
            m.serialize_entry("serial", v)?;
        }
        if let Some(v) = &self.size_bytes {
            m.serialize_entry("size_bytes", v)?;
            // Legacy alias kept by the .NET DTO (`Size => SizeBytes`).
            m.serialize_entry("size", v)?;
        }
        if let Some(v) = &self.size_gb {
            m.serialize_entry("size_gb", v)?;
        }
        if let Some(v) = &self.r#type {
            m.serialize_entry("type", v)?;
        }
        m.end()
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct GpuInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct MainboardInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct BiosInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct NetworkInterfaceInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    pub is_dual_homed: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct SoftwareInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uninstall_string: Option<String>,
    pub is_per_user: bool,
}

/// Envelope part of the v4 inventory (agent + os + security).
#[derive(Debug, Clone, Serialize)]
pub struct InventoryEnvelope {
    pub inventory_schema_version: i32,
    pub agent: AgentMetadata,
    pub os: OsMetadata,
    pub security: SecurityPostureV4,
}

/// Flat hardware/software snapshot (legacy schema fields).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InventorySnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_build: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_arch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<CpuInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ram_gb: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disks: Option<Vec<DiskInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<GpuInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mainboard: Option<MainboardInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bios: Option<BiosInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<Vec<NetworkInterfaceInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logged_user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_software: Option<Vec<SoftwareInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_vm: Option<bool>,
}
