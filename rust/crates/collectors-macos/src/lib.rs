//! macOS collectors (port of OrgInventoryAgent.MacOS/Collectors).

pub mod cmd;
pub mod fingerprint;
pub mod os;
pub mod provider;
pub mod security;
pub mod software;

pub use provider::MacosInventoryProvider;
