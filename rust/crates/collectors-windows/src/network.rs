//! Port of InventoryCollector.GetNetworkInterfaces — GetAdaptersAddresses:
//! bỏ loopback/tunnel/down; IPv4 đầu tiên else IPv6; MAC XX-XX-..;
//! ≥2 subnet IPv4 khác nhau → dual-homed, các interface subnet thứ yếu gắn is_dual_homed.

use agent_core::schema::NetworkInterfaceInfo;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, SOCKADDR_INET};

pub fn collect() -> Vec<NetworkInterfaceInfo> {
    adapters().map(to_interfaces).unwrap_or_default()
}

struct Iface {
    name: String,
    mac: String,
    is_up: bool,
    is_loopback: bool,
    is_tunnel: bool,
    /// (ip, prefix_len, is_v6)
    addrs: Vec<(std::net::IpAddr, u8, bool)>,
}

fn adapters() -> Option<Vec<Iface>> {
    unsafe {
        let mut len: u32 = 0;
        let err = GetAdaptersAddresses(
            (AF_INET.0 | AF_INET6.0) as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            None,
            None,
            &mut len,
        );
        if err != ERROR_BUFFER_OVERFLOW.0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        if GetAdaptersAddresses(
            (AF_INET.0 | AF_INET6.0) as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            None,
            Some(head),
            &mut len,
        ) != ERROR_SUCCESS.0
        {
            return None;
        }
        let mut out = Vec::new();
        let mut cur = head;
        while !cur.is_null() {
            let a = &*cur;
            let name = a.FriendlyName.to_string().unwrap_or_default();
            let mac = if a.PhysicalAddressLength > 0 {
                a.PhysicalAddress[..a.PhysicalAddressLength as usize]
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join("-")
            } else {
                String::new()
            };
            // IF_TYPE_SOFTWARE_LOOPBACK=24, IF_TYPE_TUNNEL=131; IfOperStatus Up=1.
            let if_type = a.IfType as u32;
            let mut addrs = Vec::new();
            let mut ua = a.FirstUnicastAddress;
            while !ua.is_null() {
                let u = &*ua;
                let sa = u.Address.lpSockaddr;
                if !sa.is_null() {
                    let sin = &*(sa as *const SOCKADDR_INET);
                    let fam = sa.read().sa_family;
                    if fam == AF_INET {
                        let bytes = sin.Ipv4.sin_addr.S_un.S_addr.to_be_bytes();
                        addrs.push((
                            std::net::IpAddr::V4(std::net::Ipv4Addr::from(bytes)),
                            u.OnLinkPrefixLength,
                            false,
                        ));
                    } else if fam == AF_INET6 {
                        let bytes = sin.Ipv6.sin6_addr.u.Byte;
                        addrs.push((
                            std::net::IpAddr::V6(std::net::Ipv6Addr::from(bytes)),
                            u.OnLinkPrefixLength,
                            true,
                        ));
                    }
                }
                ua = u.Next;
            }
            out.push(Iface {
                name,
                mac,
                is_up: a.OperStatus.0 == 1,
                is_loopback: if_type == 24,
                is_tunnel: if_type == 131,
                addrs,
            });
            cur = a.Next;
        }
        Some(out)
    }
}

fn subnet4(ip: &std::net::Ipv4Addr, prefix_len: u8) -> String {
    let bits = u32::from_be_bytes(ip.octets());
    let mask = if prefix_len >= 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix_len)
    };
    let net = std::net::Ipv4Addr::from((bits & mask).to_be_bytes());
    format!("{net}/{prefix_len}")
}

fn to_interfaces(ifaces: Vec<Iface>) -> Vec<NetworkInterfaceInfo> {
    let active: Vec<&Iface> = ifaces
        .iter()
        .filter(|i| i.is_up && !i.is_loopback && !i.is_tunnel)
        .collect();
    // distinct IPv4 subnets → hasDualHomed như .NET
    let subnets: Vec<String> = active
        .iter()
        .flat_map(|i| i.addrs.iter())
        .filter(|(_, _, v6)| !*v6)
        .map(|(ip, p, _)| match ip {
            std::net::IpAddr::V4(v4) => subnet4(v4, *p),
            _ => unreachable!(),
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let dual = subnets.len() >= 2;
    let primary_subnet = subnets.first().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for i in &active {
        let (ip, is_dual) = match i.addrs.iter().find(|(ip, _, v6)| !*v6 && !ip.is_loopback()) {
            Some((ip, p, _)) => {
                let ng = match ip {
                    std::net::IpAddr::V4(v4) => subnet4(v4, *p),
                    _ => String::new(),
                };
                (ip.to_string(), dual && ng != primary_subnet)
            }
            None => match i.addrs.iter().find(|(ip, _, _)| !ip.is_loopback()) {
                Some((ip, _, _)) => (ip.to_string(), false),
                None => continue,
            },
        };
        out.push(NetworkInterfaceInfo {
            name: Some(i.name.clone()),
            ip: Some(ip),
            mac: (!i.mac.is_empty()).then(|| i.mac.clone()),
            is_dual_homed: is_dual,
        });
    }
    out
}
