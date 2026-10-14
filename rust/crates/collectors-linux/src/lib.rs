//! Linux collectors (port of OrgInventoryAgent.Linux/Collectors).

#[cfg(target_os = "linux")]
pub mod cmd;
#[cfg(target_os = "linux")]
pub mod fingerprint;
#[cfg(target_os = "linux")]
pub mod os;
#[cfg(target_os = "linux")]
pub mod provider;
#[cfg(target_os = "linux")]
pub mod security;
#[cfg(target_os = "linux")]
pub mod software;

#[cfg(target_os = "linux")]
pub use provider::LinuxInventoryProvider;
