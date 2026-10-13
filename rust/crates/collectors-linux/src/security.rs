//! Port of LinuxSecurityCollector.cs (SecurityPostureV4 on Linux).

use crate::cmd::run;
use agent_core::schema::*;
use std::path::Path;
use std::time::Duration;

const SYSTEMCTL_ENABLED: [&str; 4] = ["list-unit-files", "--type=service", "--state=enabled", "-q"];

pub fn collect() -> SecurityPostureV4 {
    let enabled_units = run(
        "/usr/bin/systemctl",
        &SYSTEMCTL_ENABLED,
        Duration::from_secs(5),
    );
    SecurityPostureV4 {
        update: update_status(),
        disk_encryption: disk_encryption(),
        remote_access: enabled_units
            .as_deref()
            .map(parse_remote_access)
            .unwrap_or_default(),
        endpoint_protection: Some(endpoint_protection()),
        privilege_control: privilege_control(),
        listening_ports: agent_core::posix::ports::collect(),
        firewall_enabled: firewall_enabled(),
        local_accounts: std::fs::read_to_string("/etc/passwd")
            .ok()
            .and_then(|p| parse_accounts(&p, |u| Path::new("/etc/sudoers.d").join(u).exists())),
        startup_programs: enabled_units.as_deref().and_then(parse_startup),
        ..Default::default()
    }
}

fn update_status() -> UpdateStatus {
    if Path::new("/etc/debian_version").exists() {
        run(
            "/usr/bin/apt-get",
            &["-s", "-o", "Debug::NoLocking=true", "upgrade"],
            Duration::from_secs(10),
        )
        .map(|o| parse_apt(&o))
        .unwrap_or_else(UpdateStatus::unknown)
    } else if Path::new("/etc/redhat-release").exists() {
        run(
            "/usr/bin/dnf",
            &["check-update", "--cacheonly", "-q"],
            Duration::from_secs(10),
        )
        .map(|o| parse_dnf(&o))
        .unwrap_or_else(UpdateStatus::unknown)
    } else {
        UpdateStatus::unknown()
    }
}

pub fn parse_apt(output: &str) -> UpdateStatus {
    let Some(idx) = output.find("The following packages will be upgraded:") else {
        return UpdateStatus {
            status: "up-to-date".into(),
            pending_count: Some(0),
            security_pending_count: Some(0),
            ..UpdateStatus::unknown()
        };
    };
    let lines: Vec<&str> = output[idx..]
        .split('\n')
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    let pending = lines.len() as i32;
    let sec = lines
        .iter()
        .filter(|l| l.to_lowercase().contains("(security)"))
        .count() as i32;
    UpdateStatus {
        status: if pending > 0 {
            "updates-available"
        } else {
            "up-to-date"
        }
        .into(),
        pending_count: Some(pending),
        security_pending_count: Some(sec),
        reboot_required: Some(output.to_lowercase().contains("reboot required")),
        ..UpdateStatus::unknown()
    }
}

pub fn parse_dnf(output: &str) -> UpdateStatus {
    let pending = output.split('\n').filter(|l| !l.is_empty()).count() as i32;
    UpdateStatus {
        status: if pending > 0 {
            "updates-available"
        } else {
            "up-to-date"
        }
        .into(),
        pending_count: Some(pending),
        ..UpdateStatus::unknown()
    }
}

fn disk_encryption() -> DiskEncryptionStatus {
    match run(
        "/usr/bin/lsblk",
        &["-o", "NAME,TYPE,FSTYPE", "-J"],
        Duration::from_secs(5),
    ) {
        Some(o) => parse_lsblk(&o),
        None => DiskEncryptionStatus {
            enabled: Some(false),
            technology: Some("none".into()),
            encrypted_volumes: None,
        },
    }
}

pub fn parse_lsblk(output: &str) -> DiskEncryptionStatus {
    let lower = output.to_lowercase();
    let enabled = lower.contains("\"crypt\"") || lower.contains("crypto_luks");
    DiskEncryptionStatus {
        enabled: Some(enabled),
        technology: Some(if enabled { "luks" } else { "none" }.into()),
        encrypted_volumes: enabled.then(|| vec!["/".to_string()]),
    }
}

pub fn parse_remote_access(units: &str) -> RemoteAccessStatus {
    let lower = units.to_lowercase();
    let mut status = RemoteAccessStatus::default();
    let mut services = Vec::new();
    if lower.contains("ssh.service") || lower.contains("sshd.service") {
        status.ssh_enabled = Some(true);
        services.push("sshd".to_string());
    }
    if lower.contains("xrdp.service") || lower.contains("vncserver") {
        status.remote_desktop_enabled = Some(true);
        services.push("xrdp/vnc".to_string());
    }
    status.services = (!services.is_empty()).then_some(services);
    status
}

const KNOWN_PRODUCTS: [(&str, &[&str]); 4] = [
    ("ClamAV", &["clamd", "clamav-daemon", "clamav-milter"]),
    ("CrowdStrike Falcon", &["falcon-sensor"]),
    ("SentinelOne", &["sentinelone", "sentinel-agent"]),
    ("Wazuh", &["wazuh-agent", "wazuh-modulesd"]),
];

fn endpoint_protection() -> Vec<AntivirusInfo> {
    KNOWN_PRODUCTS
        .iter()
        .filter(|(_, procs)| {
            let pattern = procs.join("|");
            run("/usr/bin/pgrep", &["-x", &pattern], Duration::from_secs(2))
                .is_some_and(|o| !o.trim().is_empty())
        })
        .map(|(product, _)| AntivirusInfo {
            display_name: Some(product.to_string()),
            name: Some(product.to_string()),
            status: Some("enabled".into()),
            enabled: true,
            up_to_date: false,
        })
        .collect()
}

fn privilege_control() -> PrivilegeControlStatus {
    PrivilegeControlStatus {
        sudo_installed: Some(
            Path::new("/usr/bin/sudo").exists() || Path::new("/usr/sbin/sudo").exists(),
        ),
        root_account_locked: std::fs::read_to_string("/etc/shadow")
            .ok()
            .and_then(|s| parse_root_locked(&s)),
    }
}

pub fn parse_root_locked(shadow: &str) -> Option<bool> {
    let line = shadow.lines().find(|l| l.starts_with("root:"))?;
    let hash = line.split(':').nth(1)?;
    Some(hash.starts_with('!') || hash.starts_with('*'))
}

fn firewall_enabled() -> Option<bool> {
    if let Some(o) = run("/usr/sbin/ufw", &["status"], Duration::from_secs(3)) {
        let l = o.to_lowercase();
        if l.contains("status: active") {
            return Some(true);
        }
        if l.contains("status: inactive") {
            return Some(false);
        }
    }
    if let Some(o) = run(
        "/usr/bin/firewall-cmd",
        &["--state"],
        Duration::from_secs(3),
    ) {
        if o.to_lowercase().contains("running") {
            return Some(true);
        }
    }
    None
}

pub fn parse_accounts(
    passwd: &str,
    is_sudoer: impl Fn(&str) -> bool,
) -> Option<Vec<LocalAccountInfo>> {
    let list: Vec<_> = passwd
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 7 || parts[0].starts_with('_') {
                return None;
            }
            let user = parts[0];
            Some(LocalAccountInfo {
                username: Some(user.into()),
                name: Some(user.into()),
                full_name: Some(parts[4].replace(',', " ")),
                disabled: parts[6].to_lowercase().contains("nologin"),
                has_password: None,
                is_admin: is_sudoer(user),
            })
        })
        .collect();
    (!list.is_empty()).then_some(list)
}

pub fn parse_startup(units: &str) -> Option<Vec<StartupProgramInfo>> {
    let list: Vec<_> = units
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .take(200)
        .map(|l| {
            let name = l.split(' ').next().unwrap_or_default().to_string();
            StartupProgramInfo {
                name: Some(name.clone()),
                command: Some(name),
                location: Some("systemd_enabled".into()),
            }
        })
        .collect();
    (!list.is_empty()).then_some(list)
}
