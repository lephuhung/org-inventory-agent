//! Port of PosixPortCollector.cs (.NET GetActiveTcpListeners reads /proc/net/tcp{,6} state 0A).

use crate::schema::ListeningPortInfo;
use std::net::{Ipv4Addr, Ipv6Addr};

const MAX_PORTS: usize = 200;
const TCP_LISTEN: &str = "0A";

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
