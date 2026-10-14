//! Windows collectors — port of src/OrgInventoryAgent/Collectors/*.cs.
//! Mọi hàm `collect()` đều non-fatal như bản .NET: WMI/registry lỗi → trả rỗng/None.

#[cfg(windows)]
pub mod fingerprint;
#[cfg(windows)]
pub mod network;
#[cfg(windows)]
pub mod os;
#[cfg(windows)]
pub mod ports;
#[cfg(windows)]
pub mod provider;
#[cfg(windows)]
pub mod security;
#[cfg(windows)]
pub mod software;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
mod wmi;
