//! Port of PosixPortCollector.cs (.NET GetActiveTcpListeners reads /proc/net/tcp{,6} state 0A).

use crate::schema::ListeningPortInfo;
use std::net::{Ipv4Addr, Ipv6Addr};

const MAX_PORTS: usize = 200;
const TCP_LISTEN: &str = "0A";

#[cfg(not(target_os = "macos"))]
pub fn collect() -> Option<Vec<ListeningPortInfo>> {
    let mut list = Vec::new();
    for (path, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        if let Ok(content) = std::fs::read_to_string(path) {
            list.extend(parse_proc_net_tcp(&content, v6));
        }
    }
    list.truncate(MAX_PORTS);
    (!list.is_empty()).then_some(list)
}

/// macOS: no /proc — .NET reads the same TCP table that `netstat -an -p tcp` prints.
#[cfg(target_os = "macos")]
pub fn collect() -> Option<Vec<ListeningPortInfo>> {
    let out = std::process::Command::new("/usr/sbin/netstat")
        .args(["-an", "-p", "tcp"])
        .output()
        .ok()?;
    let mut list = parse_netstat_listen(&String::from_utf8_lossy(&out.stdout));
    list.truncate(MAX_PORTS);
    (!list.is_empty()).then_some(list)
}

/// Parse BSD `netstat -an -p tcp` rows like `tcp4  0  0  *.22  *.*  LISTEN`.
pub fn parse_netstat_listen(content: &str) -> Vec<ListeningPortInfo> {
    let mut seen = std::collections::HashSet::new();
    content
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 6 || !cols[0].starts_with("tcp") || cols[5] != "LISTEN" {
                return None;
            }
            let (addr, port) = cols[3].rsplit_once('.')?;
            let port: i32 = port.parse().ok()?;
            let v6 = cols[0] == "tcp6";
            let address = match addr {
                "*" if v6 => "::".to_string(),
                "*" => "0.0.0.0".to_string(),
                a => a.split('%').next().unwrap_or(a).to_string(),
            };
            seen.insert((address.clone(), port))
                .then_some(ListeningPortInfo {
                    port,
                    protocol: "TCP".into(),
                    address: Some(address),
                })
        })
        .collect()
}

pub fn parse_proc_net_tcp(content: &str, v6: bool) -> Vec<ListeningPortInfo> {
    content
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 4 || cols[3] != TCP_LISTEN {
                return None;
            }
            let (addr_hex, port_hex) = cols[1].split_once(':')?;
            let port = i32::from_str_radix(port_hex, 16).ok()?;
            let address = if v6 {
                parse_v6(addr_hex)?.to_string()
            } else {
                parse_v4(addr_hex)?.to_string()
            };
            Some(ListeningPortInfo {
                port,
                protocol: "TCP".into(),
                address: Some(address),
            })
        })
        .collect()
}

/// /proc stores the address as host-endian 32-bit words.
fn parse_v4(hex: &str) -> Option<Ipv4Addr> {
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Ipv4Addr::from(v.to_ne_bytes()))
}

fn parse_v6(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for w in 0..4 {
        let word = u32::from_str_radix(&hex[w * 8..w * 8 + 8], 16).ok()?;
        bytes[w * 4..w * 4 + 4].copy_from_slice(&word.to_ne_bytes());
    }
    Some(Ipv6Addr::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netstat_bsd_listen() {
        let out = "Active Internet connections (including servers)\nProto Recv-Q Send-Q  Local Address          Foreign Address        (state)\ntcp4       0      0  127.0.0.1.631          *.*                    LISTEN\ntcp6       0      0  *.22                   *.*                    LISTEN\ntcp4       0      0  *.22                   *.*                    LISTEN\ntcp4       0      0  192.168.1.5.52000      1.2.3.4.443            ESTABLISHED\ntcp6       0      0  fe80::1%lo0.8021       *.*                    LISTEN\n";
        let l = parse_netstat_listen(out);
        assert_eq!(l.len(), 4);
        assert_eq!(l[0].address.as_deref(), Some("127.0.0.1"));
        assert_eq!(l[0].port, 631);
        assert_eq!(l[1].address.as_deref(), Some("::"));
        assert_eq!(l[2].address.as_deref(), Some("0.0.0.0"));
        assert_eq!(l[3].address.as_deref(), Some("fe80::1"));
        assert_eq!(l[3].port, 8021);
    }

    #[test]
    fn parses_listeners_only() {
        let tcp = "  sl  local_address rem_address   st\n   0: 0100007F:0CEA 00000000:0000 0A 0\n   1: 0100007F:0CEA 0100007F:D2A4 01 0\n   2: 00000000:0016 00000000:0000 0A 0\n";
        let r = parse_proc_net_tcp(tcp, false);
        assert_eq!(r.len(), 2);
        assert_eq!(
            (r[0].port, r[0].address.as_deref()),
            (3306, Some("127.0.0.1"))
        );
        assert_eq!((r[1].port, r[1].address.as_deref()), (22, Some("0.0.0.0")));
    }

    #[test]
    fn parses_v6() {
        let tcp6 = "hdr\n 0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 0\n 1: 00000000000000000000000000000000:0050 00000000000000000000000000000000:0000 0A 0\n";
        let r = parse_proc_net_tcp(tcp6, true);
        assert_eq!(r[0].address.as_deref(), Some("::1"));
        assert_eq!(r[0].port, 8080);
        assert_eq!(r[1].address.as_deref(), Some("::"));
    }
}
