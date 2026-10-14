//! Port of AgentState (OfflineCache.cs) — `{DataDir}/state.json`, camelCase.

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_inventory_at: Option<DateTime<FixedOffset>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_inventory_config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_agent_config_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_update_check_at: Option<DateTime<FixedOffset>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl AgentState {
    pub fn load_from(path: PathBuf) -> Self {
        let mut s = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| {
                // case-insensitive like System.Text.Json
                let v: serde_json::Value = serde_json::from_str(&t).ok()?;
                let obj = v.as_object()?;
                let mut m = serde_json::Map::new();
                for (k, val) in obj {
                    for known in [
                        "lastInventoryAt",
                        "lastInventoryConfigHash",
                        "lastAgentConfigHash",
                        "lastUpdateCheckAt",
                    ] {
                        if known.eq_ignore_ascii_case(k) && !val.is_null() {
                            m.insert(known.into(), val.clone());
                        }
                    }
                }
                serde_json::from_value::<AgentState>(serde_json::Value::Object(m)).ok()
            })
            .unwrap_or_default();
        s.path = Some(path);
        s
    }

    pub fn load() -> Self {
        Self::load_from(crate::paths::state_file())
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = crate::config::write_atomic(path, json.as_bytes(), 0o600) {
                    log::warn!(target: "AgentState", "Không lưu được state.json: {e}");
                }
            }
            Err(e) => log::warn!(target: "AgentState", "Serialize state lỗi: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut s = AgentState::load_from(p.clone());
        s.last_agent_config_hash = Some("abc".into());
        s.last_inventory_at = Some(chrono::Utc::now().fixed_offset());
        s.save();
        let r = AgentState::load_from(p.clone());
        assert_eq!(r.last_agent_config_hash.as_deref(), Some("abc"));
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("lastAgentConfigHash"));
    }
}
