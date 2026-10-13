//! Port of AgentIdentity.cs — enrollment state machine.

use crate::config::AgentConfig;
use crate::keystore::KeyStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollStatus {
    NotEnrolled,
    Enrolled,
    /// Previously enrolled but the cert vanished (or flagged): wait for a fresh admin token,
    /// never silently fresh-enroll (keeps the original machine_id).
    ReenrollRequired,
}

fn blank(s: &Option<String>) -> bool {
    s.as_deref().is_none_or(|v| v.trim().is_empty())
}

/// Fast config-only check (does not touch the cert files).
pub fn is_enrolled(c: &AgentConfig) -> bool {
    c.enrolled && !blank(&c.machine_id) && !blank(&c.client_cert_thumbprint)
}

pub fn validate(c: &mut AgentConfig, ks: &KeyStore) -> EnrollStatus {
    if c.reenroll_required {
        return EnrollStatus::ReenrollRequired;
    }
    if !is_enrolled(c) {
        return EnrollStatus::NotEnrolled;
    }
    if ks.find(c).is_some() {
        EnrollStatus::Enrolled
    } else {
        EnrollStatus::ReenrollRequired
    }
}

/// True while waiting for a fresh token after ReenrollRequired.
pub fn is_reenroll_pending(c: &AgentConfig) -> bool {
    if is_enrolled(c) {
        return false;
    }
    if c.reenroll_required && !blank(&c.token) {
        return false;
    }
    c.reenroll_required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reenroll_pending_logic() {
        let mut c = AgentConfig {
            reenroll_required: true,
            machine_id: Some("m".into()),
            ..Default::default()
        };
        assert!(is_reenroll_pending(&c));
        c.token = Some("fresh".into());
        assert!(!is_reenroll_pending(&c));
        let e = AgentConfig {
            enrolled: true,
            machine_id: Some("m".into()),
            client_cert_thumbprint: Some("AA".into()),
            reenroll_required: true,
            ..Default::default()
        };
        assert!(is_enrolled(&e) && !is_reenroll_pending(&e));
    }
}
