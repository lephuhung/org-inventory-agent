//! Port of EndpointManager.cs — primary/backup failover.

use crate::SharedConfig;
use std::sync::Mutex;

const FAIL_THRESHOLD: u32 = 5;
const RETRY_PRIMARY_EVERY: u32 = 10;

#[derive(Default)]
struct State {
    current: usize,
    consecutive_failures: u32,
    cycle_count: u32,
}

pub struct EndpointManager {
    config: SharedConfig,
    state: Mutex<State>,
}

impl EndpointManager {
    pub fn new(config: SharedConfig) -> Self {
        Self {
            config,
            state: Mutex::new(State::default()),
        }
    }

    fn endpoints(&self) -> Vec<String> {
        self.config.lock().unwrap().endpoints.clone()
    }

    pub fn current(&self) -> Option<String> {
        let eps = self.endpoints();
        let st = self.state.lock().unwrap();
        (!eps.is_empty()).then(|| eps[st.current % eps.len()].clone())
    }

    pub fn on_success(&self) {
        let mut st = self.state.lock().unwrap();
        st.consecutive_failures = 0;
        st.cycle_count += 1;
    }

    pub fn on_failure(&self) {
        let eps = self.endpoints();
        let mut st = self.state.lock().unwrap();
        st.consecutive_failures += 1;
        st.cycle_count += 1;
        if eps.len() <= 1 {
            return;
        }
        st.current %= eps.len();
        if st.consecutive_failures >= FAIL_THRESHOLD {
            let from = st.current;
            st.current = (st.current + 1) % eps.len();
            st.consecutive_failures = 0;
            log::warn!(target: "EndpointManager", "Endpoint {} lỗi {FAIL_THRESHOLD} lần liên tiếp → chuyển sang {}", eps[from], eps[st.current]);
            return;
        }
        if st.current != 0 && st.cycle_count.is_multiple_of(RETRY_PRIMARY_EVERY) {
            log::info!(target: "EndpointManager", "Thử lại endpoint primary (chu kỳ {})", st.cycle_count);
            st.current = 0;
            st.consecutive_failures = 0;
        }
    }

    pub fn build_url(&self, path: &str) -> anyhow::Result<String> {
        let base = self
            .current()
            .ok_or_else(|| anyhow::anyhow!("Chưa có endpoint server."))?;
        Ok(format!("{}{path}", base.trim_end_matches('/')))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AgentConfig;
    use std::sync::{Arc, Mutex};

    fn mgr(eps: &[&str]) -> EndpointManager {
        let cfg = AgentConfig {
            endpoints: eps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        EndpointManager::new(Arc::new(Mutex::new(cfg)))
    }

    #[test]
    fn fails_over_after_five_and_retries_primary() {
        let m = mgr(&["https://p", "https://b"]);
        for _ in 0..4 {
            m.on_failure();
        }
        assert_eq!(m.current().as_deref(), Some("https://p"));
        m.on_failure();
        assert_eq!(m.current().as_deref(), Some("https://b"));
        for _ in 0..4 {
            m.on_failure();
        }
        assert_eq!(m.current().as_deref(), Some("https://b"));
        m.on_failure(); // cycle 10 → back to primary
        assert_eq!(m.current().as_deref(), Some("https://p"));
    }

    #[test]
    fn success_resets_and_single_endpoint_never_moves() {
        let m = mgr(&["https://only/"]);
        for _ in 0..20 {
            m.on_failure();
        }
        assert_eq!(
            m.build_url("/api/heartbeat").unwrap(),
            "https://only/api/heartbeat"
        );
        let m = mgr(&["https://p", "https://b"]);
        for _ in 0..4 {
            m.on_failure();
        }
        m.on_success();
        m.on_failure();
        assert_eq!(m.current().as_deref(), Some("https://p"));
    }
}
