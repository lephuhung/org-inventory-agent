//! Port of MacosSecurityCollector.cs (SecurityPostureV4 for macOS).

use crate::cmd;
use agent_core::schema::{
    AntivirusInfo, DiskEncryptionStatus, LocalAccountInfo, PrivilegeControlStatus,
    RemoteAccessStatus, SecurityPostureV4, StartupProgramInfo, UpdateStatus,
};
use std::collections::HashSet;

fn has(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

pub fn collect() -> SecurityPostureV4 {
    SecurityPostureV4 {
        update: update(),
        disk_encryption: disk_encryption(),
        remote_access: remote_access(),
        endpoint_protection: Some(endpoint_protection()),
        privilege_control: privilege_control(),
        listening_ports: agent_core::posix::ports::collect(),
        firewall_enabled: firewall(),
        local_accounts: accounts(),
        startup_programs: startup(),
        ..Default::default()
    }
}

pub fn update() -> UpdateStatus {
    match cmd::run("/usr/sbin/softwareupdate", &["-l"], 90_000) {
        Some(out) => parse_softwareupdate(&out),
        None => UpdateStatus {
            status: "unknown".into(),
            ..Default::default()
        },
    }
}

pub fn parse_softwareupdate(output: &str) -> UpdateStatus {
    if has(output, "No new software available") {
        return UpdateStatus {
            status: "up-to-date".into(),
            pending_count: Some(0),
            security_pending_count: Some(0),
            ..Default::default()
        };
    }
    let pending = output
        .lines()
        .filter(|l| l.trim_start().starts_with("* Label:"))
        .count() as i32;
    let sec = output
        .lines()
        .filter(|l| has(l, "Recommended:") && has(l, "YES"))
        .count() as i32;
    let reboot = has(output, "Action: restart") || has(output, "[restart]");
    UpdateStatus {
        status: if pending > 0 {
            "updates-available"
        } else {
            "up-to-date"
        }
        .into(),
        pending_count: Some(pending),
        security_pending_count: (sec > 0).then_some(sec),
        reboot_required: reboot.then_some(true),
        ..Default::default()
    }
}

pub fn disk_encryption() -> DiskEncryptionStatus {
    parse_fdesetup(cmd::run("/usr/bin/fdesetup", &["status"], 10_000).as_deref())
}

pub fn parse_fdesetup(output: Option<&str>) -> DiskEncryptionStatus {
    let Some(out) = output else {
        return DiskEncryptionStatus::default();
    };
    let on = has(out, "FileVault is On");
    if !on && !has(out, "FileVault is Off") {
        return DiskEncryptionStatus::default();
    }
    DiskEncryptionStatus {
        enabled: Some(on),
        technology: Some("filevault".into()),
        encrypted_volumes: on.then(|| vec!["/".to_string()]),
    }
}

pub fn remote_access() -> RemoteAccessStatus {
    let remote_login = cmd::run("/usr/sbin/systemsetup", &["-getremotelogin"], 10_000);
    let need_launchctl = !remote_login
        .as_deref()
        .is_some_and(|s| has(s, "Remote Login:"));
    let sshd = need_launchctl
        .then(|| {
            cmd::run(
                "/bin/launchctl",
                &["print", "system/com.openssh.sshd"],
                10_000,
            )
        })
        .flatten();
    let screensharing = cmd::run(
        "/bin/launchctl",
        &["print", "system/com.apple.screensharing"],
        10_000,
    );
    parse_remote_access(
        remote_login.as_deref(),
        sshd.as_deref(),
        screensharing.as_deref(),
    )
}

/// systemsetup needs root (non-root prints "administrator access" with exit 0) →
/// fall back to `launchctl print system/com.openssh.sshd`.
pub fn parse_remote_access(
    remote_login: Option<&str>,
    sshd: Option<&str>,
    screensharing: Option<&str>,
) -> RemoteAccessStatus {
    let mut st = RemoteAccessStatus::default();
    let mut services = Vec::new();
    match remote_login.filter(|s| has(s, "Remote Login:")) {
        Some(rl) => {
            let on = has(rl, "Remote Login: On");
            st.ssh_enabled = Some(on);
            if on {
                services.push("ssh (Remote Login)".to_string());
            }
        }
        None => {
            let on = sshd.is_some_and(|s| !has(s, "Could not find service"));
            st.ssh_enabled = Some(on);
            if on {
                services.push("sshd".to_string());
            }
        }
    }
    if let Some(ss) = screensharing {
        let on = has(ss, "state = running");
        st.remote_desktop_enabled = Some(on);
        if on {
            services.push("screensharing".to_string());
        }
    }
    st.services = (!services.is_empty()).then_some(services);
    st
}

const KNOWN_PRODUCTS: [(&str, &[&str]); 5] = [
    (
        "CrowdStrike Falcon",
        &["falcond", "com.crowdstrike.falcon.Agent"],
    ),
    ("SentinelOne", &["sentineld", "sentinel-agent"]),
    ("Jamf Protect", &["JamfProtect", "jamfprotect"]),
    ("Microsoft Defender", &["wdavdaemon", "mdatp"]),
    ("osquery", &["osqueryd"]),
];

pub fn endpoint_protection() -> Vec<AntivirusInfo> {
    // XProtect is macOS's built-in anti-malware — always active.
    let mut found = vec![AntivirusInfo {
        display_name: Some("XProtect (built-in)".into()),
        name: Some("XProtect".into()),
        enabled: true,
        status: Some("enabled".into()),
        up_to_date: true,
    }];
    for (product, procs) in KNOWN_PRODUCTS {
        let pattern = procs.join("|");
        if cmd::run("/usr/bin/pgrep", &["-fl", &pattern], 3_000).is_some() {
            found.push(AntivirusInfo {
                display_name: Some(product.into()),
                name: Some(product.into()),
                enabled: true,
                status: Some("enabled".into()),
                up_to_date: false,
            });
        }
    }
    found
}

pub fn privilege_control() -> PrivilegeControlStatus {
    let auth = cmd::run(
        "/usr/bin/dscl",
        &[".", "-read", "/Users/root", "AuthenticationAuthority"],
        10_000,
    );
    let decided = parse_root_auth(auth.as_deref());
    let root_account_locked = decided.or_else(|| {
        // "No such key" goes to stderr → read the full record: ShadowHashData = root enabled.
        cmd::run("/usr/bin/dscl", &[".", "-read", "/Users/root"], 10_000)
            .map(|r| !has(&r, "ShadowHashData"))
    });
    PrivilegeControlStatus {
        sudo_installed: Some(std::path::Path::new("/usr/bin/sudo").exists()),
        root_account_locked,
    }
}

/// Some(true) = DisabledUser, Some(false) = has a password hash, None = undecided.
pub fn parse_root_auth(auth: Option<&str>) -> Option<bool> {
    let a = auth?;
    if has(a, "DisabledUser") {
        Some(true)
    } else if has(a, "ShadowHash") || has(a, "SaltedSHA") {
        Some(false)
    } else {
        None
    }
}

pub fn firewall() -> Option<bool> {
    parse_firewall(&cmd::run(
        "/usr/libexec/ApplicationFirewall/socketfilterfw",
        &["--getglobalstate"],
        10_000,
    )?)
}

pub fn parse_firewall(out: &str) -> Option<bool> {
    if has(out, "enabled") {
        Some(true)
    } else if has(out, "disabled") {
        Some(false)
    } else {
        None
    }
}

pub fn accounts() -> Option<Vec<LocalAccountInfo>> {
    let users = cmd::run(
        "/usr/bin/dscl",
        &[".", "-list", "/Users", "UniqueID"],
        10_000,
    )?;
    let admins = parse_group_membership(
        &cmd::run(
            "/usr/bin/dscl",
            &[".", "-read", "/Groups/admin", "GroupMembership"],
            10_000,
        )
        .unwrap_or_default(),
    );
    let mut list = Vec::new();
    for (name, uid) in parse_users_unique_id(&users) {
        if uid < 500 || name.starts_with('_') {
            continue;
        }
        let user_path = format!("/Users/{name}");
        let full_name = cmd::run(
            "/usr/bin/dscl",
            &[".", "-read", &user_path, "RealName"],
            10_000,
        )
        .and_then(|s| s.split_once(':').map(|(_, v)| v.trim().to_string()))
        .filter(|s| !s.is_empty());
        let disabled = cmd::run(
            "/usr/bin/dscl",
            &[".", "-read", &user_path, "AuthenticationAuthority"],
            3_000,
        )
        .is_some_and(|a| has(&a, "DisabledUser"));
        list.push(LocalAccountInfo {
            username: Some(name.clone()),
            is_admin: admins.contains(&name.to_lowercase()),
            name: Some(name),
            full_name,
            disabled,
            ..Default::default()
        });
        if list.len() >= 200 {
            break;
        }
    }
    (!list.is_empty()).then_some(list)
}

pub fn parse_users_unique_id(output: &str) -> Vec<(String, i32)> {
    output
        .lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() != 2 {
                return None;
            }
            Some((parts[0].to_string(), parts[1].parse().ok()?))
        })
        .collect()
}

/// "dsAttrTypeStandard:GroupMembership: admin root operator" → lowercase member set.
pub fn parse_group_membership(output: &str) -> HashSet<String> {
    output
        .rfind(':')
        .map(|i| {
            output[i + 1..]
                .split_whitespace()
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

pub fn startup() -> Option<Vec<StartupProgramInfo>> {
    let mut list = Vec::new();
    for (dir, location) in [
        ("/Library/LaunchDaemons", "LaunchDaemon"),
        ("/Library/LaunchAgents", "LaunchAgent"),
    ] {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "plist"))
            .collect();
        files.sort();
        for p in files {
            list.push(StartupProgramInfo {
                name: p.file_stem().map(|s| s.to_string_lossy().into_owned()),
                command: Some(p.to_string_lossy().into_owned()),
                location: Some(location.into()),
            });
            if list.len() >= 200 {
                return Some(list);
            }
        }
    }
    (!list.is_empty()).then_some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softwareupdate_up_to_date() {
        let st = parse_softwareupdate(
            "Software Update Tool\n\nFinding available software\nNo new software available.\n",
        );
        assert_eq!(st.status, "up-to-date");
        assert_eq!(st.pending_count, Some(0));
    }

    #[test]
    fn softwareupdate_counts_labels() {
        let out = "Software Update Tool\nFinding available software\nSoftware Update found the following new or updated software:\n* Label: macOS Sonoma 14.6-23G80\n* Label: Safari17.6\n";
        let st = parse_softwareupdate(out);
        assert_eq!(st.status, "updates-available");
        assert_eq!(st.pending_count, Some(2));
        assert_eq!(st.security_pending_count, None);
        assert_eq!(st.reboot_required, None);
    }

    #[test]
    fn softwareupdate_real_format_recommended_and_restart() {
        let out = "Software Update Tool\nFinding available software\nSoftware Update found the following new or updated software:\n* Label: macOS 26.6.3-25G212\n\tTitle: macOS 26.6.3, Version: 26.6.3, Size: 3145728K, Recommended: YES, Action: restart,\n* Label: Safari26.0.1\n\tTitle: Safari, Version: 26.0.1, Size: 102400K, Recommended: YES,\n";
        let st = parse_softwareupdate(out);
        assert_eq!(st.pending_count, Some(2));
        assert_eq!(st.security_pending_count, Some(2));
        assert_eq!(st.reboot_required, Some(true));
    }

    #[test]
    fn users_unique_id() {
        let l = parse_users_unique_id("daemon        1\nadmin         501\n_guest        201\n");
        assert_eq!(l.len(), 3);
        assert!(l.contains(&("admin".to_string(), 501)));
    }

    #[test]
    fn group_membership() {
        let s = parse_group_membership("dsAttrTypeStandard:GroupMembership: admin root operator\n");
        assert_eq!(s.len(), 3);
        assert!(s.contains("admin") && s.contains("root"));
    }

    #[test]
    fn fdesetup() {
        let on = parse_fdesetup(Some("FileVault is On.\n"));
        assert_eq!(on.enabled, Some(true));
        assert_eq!(on.encrypted_volumes, Some(vec!["/".to_string()]));
        let off = parse_fdesetup(Some("FileVault is Off.\n"));
        assert_eq!(off.enabled, Some(false));
        assert_eq!(off.encrypted_volumes, None);
        assert_eq!(parse_fdesetup(Some("weird")).enabled, None);
    }

    #[test]
    fn remote_access_variants() {
        let r = parse_remote_access(Some("Remote Login: On\n"), None, Some("state = running"));
        assert_eq!(r.ssh_enabled, Some(true));
        assert_eq!(r.remote_desktop_enabled, Some(true));
        assert_eq!(
            r.services,
            Some(vec![
                "ssh (Remote Login)".to_string(),
                "screensharing".to_string()
            ])
        );
        let r = parse_remote_access(
            Some("You need administrator access to run this tool"),
            Some("Could not find service \"com.openssh.sshd\""),
            None,
        );
        assert_eq!(r.ssh_enabled, Some(false));
        assert_eq!(r.remote_desktop_enabled, None);
        assert_eq!(r.services, None);
        let r = parse_remote_access(None, Some("system/com.openssh.sshd = {"), None);
        assert_eq!(r.ssh_enabled, Some(true));
        assert_eq!(r.services, Some(vec!["sshd".to_string()]));
    }

    #[test]
    fn root_auth_and_firewall() {
        assert_eq!(
            parse_root_auth(Some("AuthenticationAuthority: ;DisabledUser;")),
            Some(true)
        );
        assert_eq!(
            parse_root_auth(Some("AuthenticationAuthority: ;ShadowHash;HASHLIST")),
            Some(false)
        );
        assert_eq!(parse_root_auth(Some("")), None);
        assert_eq!(
            parse_firewall("Firewall is enabled. (State = 1)"),
            Some(true)
        );
        assert_eq!(
            parse_firewall("Firewall is disabled. (State = 0)"),
            Some(false)
        );
        assert_eq!(parse_firewall("???"), None);
    }
}
