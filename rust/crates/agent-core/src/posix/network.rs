//! Port of PosixNetworkCollector.cs + PosixPrimaryIp.cs.

use crate::schema::NetworkInterfaceInfo;
use std::collections::HashSet;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq)]
pub struct IfaceEntry {
    pub name: String,
    pub ip: Option<Ipv4Addr>,
    pub mask: Option<Ipv4Addr>,
    pub mac: Option<String>,
}

/// Active (operstate up), non-loopback interfaces in enumeration order.
pub fn active_interfaces() -> Vec<IfaceEntry> {
    let Ok(addrs) = nix::ifaddrs::getifaddrs() else {
        return Vec::new();
    };
    let mut out: Vec<IfaceEntry> = Vec::new();
    for a in addrs {
        let name = a.interface_name.clone();
        if !is_up_non_loopback(&name, a.flags) {
            continue;
        }
        let v4 = a
            .address
            .as_ref()
            .and_then(|s| s.as_sockaddr_in())
            .map(|s| s.ip());
        let mask = a
            .netmask
            .as_ref()
            .and_then(|s| s.as_sockaddr_in())
            .map(|s| s.ip());
        match out.iter_mut().find(|e| e.name == name) {
            Some(e) => {
                if e.ip.is_none() && v4.is_some() {
                    e.ip = v4;
                    e.mask = mask;
                }
            }
            None => out.push(IfaceEntry {
                mac: read_mac(&name),
                name,
                ip: v4,
                mask: if v4.is_some() { mask } else { None },
            }),
        }
    }
    out
}

fn is_up_non_loopback(name: &str, flags: nix::net::if_::InterfaceFlags) -> bool {
    use nix::net::if_::InterfaceFlags;
    if flags.contains(InterfaceFlags::IFF_LOOPBACK) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        // .NET maps /sys/class/net/<if>/operstate; only "up" counts as OperationalStatus.Up.
        if let Ok(state) = std::fs::read_to_string(format!("/sys/class/net/{name}/operstate")) {
            return state.trim() == "up";
        }
    }
    let _ = name;
    flags.contains(InterfaceFlags::IFF_UP) && flags.contains(InterfaceFlags::IFF_RUNNING)
}

fn read_mac(name: &str) -> Option<String> {
    let raw = std::fs::read_to_string(format!("/sys/class/net/{name}/address")).ok()?;
    format_mac(raw.trim())
}

/// "aa:bb:cc:dd:ee:ff" -> "AA-BB-CC-DD-EE-FF" (.NET PhysicalAddress formatting).
pub fn format_mac(raw: &str) -> Option<String> {
    let parts: Vec<&str> = raw.split(':').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    Some(
        parts
            .iter()
            .map(|p| format!("{:0>2}", p.to_uppercase()))
            .collect::<Vec<_>>()
            .join("-"),
    )
}

pub fn subnet_group(ip: Ipv4Addr, mask: Option<Ipv4Addr>) -> String {
    let o = ip.octets();
    match mask {
        Some(m) => {
            let mo = m.octets();
            let prefix: u32 = mo.iter().map(|b| b.count_ones()).sum();
            format!(
                "{}.{}.{}.{}/{}",
                o[0] & mo[0],
                o[1] & mo[1],
                o[2] & mo[2],
                o[3] & mo[3],
                prefix
            )
        }
        None => format!("{}.{}.0.0/16", o[0], o[1]),
    }
}

/// Dual-homed: >= 2 distinct IPv4 subnets; every interface not on the first subnet is flagged.
pub fn build(entries: &[IfaceEntry]) -> Option<Vec<NetworkInterfaceInfo>> {
    let groups: Vec<Option<String>> = entries
        .iter()
        .map(|e| e.ip.map(|ip| subnet_group(ip, e.mask)))
        .collect();
    let distinct: HashSet<String> = groups.iter().flatten().map(|g| g.to_lowercase()).collect();
    let dual = distinct.len() >= 2;
    let mut first: Option<&String> = None;
    let out: Vec<_> = entries
        .iter()
        .zip(&groups)
        .map(|(e, g)| {
            let mut secondary = false;
            if dual {
                if let Some(g) = g {
                    let f = *first.get_or_insert(g);
                    secondary = !f.eq_ignore_ascii_case(g);
                }
            }
            NetworkInterfaceInfo {
                name: Some(e.name.clone()),
                ip: e.ip.map(|i| i.to_string()),
                mac: e.mac.clone(),
                is_dual_homed: dual && secondary,
            }
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

pub fn collect() -> Option<Vec<NetworkInterfaceInfo>> {
    build(&active_interfaces())
}

pub fn primary_ip() -> String {
    active_interfaces()
        .into_iter()
        .find_map(|e| e.ip)
        .map(|i| i.to_string())
        .unwrap_or_else(|| "127.0.0.1".into())
}
