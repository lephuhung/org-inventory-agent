//! WMI helper — mỗi collector mở `WMIConnection` (COM init trên thread caller).
//! Gọi từ tokio `spawn_blocking` hoặc thread đồng bộ — KHÔNG share connection qua thread.

use serde::de::DeserializeOwned;
pub use wmi::WMIConnection;

pub fn conn() -> Option<WMIConnection> {
    match wmi::COMLibrary::new().and_then(WMIConnection::new) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!(target: "WMI", "WMIConnection::new failed: {e}");
            None
        }
    }
}

pub fn conn_ns(ns: &str) -> Option<WMIConnection> {
    match wmi::COMLibrary::new().and_then(|com| WMIConnection::with_namespace_path(ns, com)) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!(target: "WMI", "WMIConnection({ns}) failed: {e}");
            None
        }
    }
}

/// `c.raw_query::<T>(wql)` — rỗng khi lỗi (non-fatal như .NET).
pub fn query<T: DeserializeOwned>(c: &WMIConnection, wql: &str) -> Vec<T> {
    c.raw_query(wql).unwrap_or_default()
}
