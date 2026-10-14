//! Port of LinuxSoftwareCollector.cs: dpkg / rpm package inventory, capped at 500 entries.

use crate::cmd::run;
use agent_core::schema::SoftwareInfo;
use std::path::Path;
use std::time::Duration;

const MAX_PACKAGES: usize = 500;

pub fn collect() -> Vec<SoftwareInfo> {
    let out = if Path::new("/etc/debian_version").exists() {
        run(
            "/usr/bin/dpkg-query",
            &["-W", "-f=${Package}\\t${Version}\\t${Maintainer}\\n"],
            Duration::from_secs(15),
        )
    } else if Path::new("/etc/redhat-release").exists() {
        run(
            "/usr/bin/rpm",
            &[
                "-qa",
                "--queryformat",
                "%{NAME}\\t%{VERSION}\\t%{VENDOR}\\n",
            ],
            Duration::from_secs(15),
        )
    } else {
        None
    };
    out.map(|o| parse_tab_separated(&o)).unwrap_or_default()
}

/// Lines of `name\tversion\tpublisher`.
pub fn parse_tab_separated(output: &str) -> Vec<SoftwareInfo> {
    output
        .split('\n')
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 3 || parts[0].trim().is_empty() {
                return None;
            }
            Some(SoftwareInfo {
                display_name: Some(parts[0].into()),
                name: Some(parts[0].into()),
                version: Some(parts[1].into()),
                publisher: Some(parts[2].into()),
                is_per_user: false,
                ..Default::default()
            })
        })
        .take(MAX_PACKAGES)
        .collect()
}
