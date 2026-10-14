//! Port of PortCollector.cs — TCP listeners. Windows chạy `netstat -an -p tcp`
//! rồi parse (không cần crate ngoài).

use agent_core::schema::ListeningPortInfo;
use std::collections::BTreeMap;

pub fn collect() -> Vec<ListeningPortInfo> {
    let out = std::process::Command::new("netstat")
        .args(["-an", "-p", "tcp"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    parse_netstat(&out)
}

/// Dòng netstat: `  TCP    0.0.0.0:135    0.0.0.0:0    LISTENING`
fn parse_netstat(text: &str) -> Vec<ListeningPortInfo> {
    let mut ports = BTreeMap::new();
    for line in text.lines() {
        let l = line.trim();
        if !l.starts_with("TCP") {
            continue;
        }
        let cols: Vec<&str> = l.split_whitespace().collect();
        if cols.len() < 4 || !cols[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        let Some((addr, port)) = cols[1].rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port.parse::<i32>() else {
            continue;
        };
        let addr = match addr {
            "0.0.0.0" | "[::]" | "::" => "*".to_string(),
            a => a.trim_start_matches('[').trim_end_matches(']').to_string(),
        };
        ports.entry(port).or_insert(ListeningPortInfo {
            port,
            address: Some(addr),
            protocol: "TCP".into(),
        });
    }
    let mut v: Vec<_> = ports.into_values().collect();
    v.truncate(100);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listening() {
        let text = "  TCP    0.0.0.0:135    0.0.0.0:0    LISTENING\n  TCP    192.168.1.5:5000    0.0.0.0:0    LISTENING\n  TCP    0.0.0.0:135    0.0.0.0:0    TIME_WAIT\n";
        let p = parse_netstat(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].port, 135);
        assert_eq!(p[0].address.as_deref(), Some("*"));
        assert_eq!(p[1].address.as_deref(), Some("192.168.1.5"));
    }
}
