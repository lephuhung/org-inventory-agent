//! macOS collectors (port of OrgInventoryAgent.MacOS/Collectors).

#[cfg(target_os = "macos")]
pub mod cmd;
#[cfg(target_os = "macos")]
pub mod fingerprint;
#[cfg(target_os = "macos")]
pub mod os;
#[cfg(target_os = "macos")]
pub mod provider;
#[cfg(target_os = "macos")]
pub mod security;
#[cfg(target_os = "macos")]
pub mod software;

#[cfg(target_os = "macos")]
pub use provider::MacosInventoryProvider;
