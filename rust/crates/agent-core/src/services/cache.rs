//! Port of OfflineCache.cs — SQLite `cache.db`, dedupe by (url, sha256(body)), 7-day retention,
//! drop after 10 attempts. If the DB cannot be opened the cache is a no-op (like `_conn = null`).

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Mutex;

pub const MAX_ATTEMPTS: i64 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct PendingItem {
    pub id: i64,
    pub url: String,
    pub body: String,
    pub attempts: i64,
}

pub struct OfflineCache {
    conn: Mutex<Option<Connection>>,
}

fn now_o() -> String {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
        .replace('Z', "+00:00")
}

impl OfflineCache {
    pub fn open(path: &Path) -> Self {
        let conn = (|| -> rusqlite::Result<Connection> {
            let c = Connection::open(path)?;
            c.execute_batch(
                "CREATE TABLE IF NOT EXISTS pending (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    url TEXT NOT NULL,
                    body TEXT NOT NULL,
                    body_hash TEXT NOT NULL,
                    attempts INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );
                CREATE UNIQUE INDEX IF NOT EXISTS ux_pending_body ON pending(url, body_hash);",
            )?;
            Ok(c)
        })();
        let conn = match conn {
            Ok(c) => Some(c),
            Err(e) => {
                log::error!(target: "OfflineCache", "Không mở được SQLite cache ({}): {e}", path.display());
                None
            }
        };
        let cache = Self {
            conn: Mutex::new(conn),
        };
        cache.cleanup_old(7);
        cache
    }

    pub fn available(&self) -> bool {
        self.conn.lock().unwrap().is_some()
    }

    pub fn cleanup_old(&self, max_days: i64) -> usize {
        let g = self.conn.lock().unwrap();
        let Some(c) = g.as_ref() else { return 0 };
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(max_days))
            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
            .replace('Z', "+00:00");
        match c.execute("DELETE FROM pending WHERE created_at < ?1", params![cutoff]) {
            Ok(n) => {
                if n > 0 {
                    log::info!(target: "OfflineCache", "Đã dọn dẹp {n} bản ghi offline cache cũ (> {max_days} ngày).");
                }
                n
            }
            Err(e) => {
                log::warn!(target: "OfflineCache", "Dọn dẹp offline cache cũ lỗi: {e}");
                0
            }
        }
    }

    pub fn enqueue(&self, url: &str, body: &str) {
        let g = self.conn.lock().unwrap();
        let Some(c) = g.as_ref() else { return };
        let hash = hex::encode(Sha256::digest(body.as_bytes()));
        match c.execute(
            "INSERT OR IGNORE INTO pending (url, body, body_hash, attempts, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
            params![url, body, hash, now_o()],
        ) {
            Ok(_) => log::info!(target: "OfflineCache", "Đã lưu offline cache: {url} ({})", &hash[..8]),
            Err(e) => log::error!(target: "OfflineCache", "Enqueue offline cache lỗi: {e}"),
        }
    }

    pub fn get_all(&self) -> Vec<PendingItem> {
        let g = self.conn.lock().unwrap();
        let Some(c) = g.as_ref() else { return vec![] };
        let r = (|| -> rusqlite::Result<Vec<PendingItem>> {
            let mut st = c.prepare("SELECT id, url, body, attempts FROM pending ORDER BY id")?;
            let rows = st.query_map([], |r| {
                Ok(PendingItem {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    body: r.get(2)?,
                    attempts: r.get(3)?,
                })
            })?;
            rows.collect()
        })();
        r.unwrap_or_else(|e| {
            log::error!(target: "OfflineCache", "Đọc offline cache lỗi: {e}");
            vec![]
        })
    }

    pub fn count(&self) -> i64 {
        let g = self.conn.lock().unwrap();
        g.as_ref()
            .and_then(|c| {
                c.query_row("SELECT COUNT(*) FROM pending", [], |r| r.get(0))
                    .ok()
            })
            .unwrap_or(0)
    }

    /// Returns true when the item reached MAX_ATTEMPTS and should be dropped.
    pub fn increment_attempts(&self, id: i64) -> bool {
        let g = self.conn.lock().unwrap();
        let Some(c) = g.as_ref() else { return true };
        let r = (|| -> rusqlite::Result<i64> {
            c.execute(
                "UPDATE pending SET attempts = attempts + 1 WHERE id = ?1",
                params![id],
            )?;
            c.query_row(
                "SELECT attempts FROM pending WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
        })();
        r.map_or(true, |a| a >= MAX_ATTEMPTS)
    }

    pub fn delete(&self, id: i64) {
        let g = self.conn.lock().unwrap();
        if let Some(c) = g.as_ref() {
            let _ = c.execute("DELETE FROM pending WHERE id = ?1", params![id]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_attempts_delete_and_retention() {
        let dir = tempfile::tempdir().unwrap();
        let cache = OfflineCache::open(&dir.path().join("cache.db"));
        assert!(cache.available());
        cache.enqueue("https://s/api/inventory", r#"{"a":1}"#);
        cache.enqueue("https://s/api/inventory", r#"{"a":1}"#);
        cache.enqueue("https://s/api/inventory", r#"{"a":2}"#);
        let all = cache.get_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].body, r#"{"a":1}"#);
        for i in 1..MAX_ATTEMPTS {
            assert!(!cache.increment_attempts(all[0].id), "attempt {i}");
        }
        assert!(cache.increment_attempts(all[0].id));
        cache.delete(all[0].id);
        assert_eq!(cache.count(), 1);
        {
            let g = cache.conn.lock().unwrap();
            g.as_ref()
                .unwrap()
                .execute(
                    "UPDATE pending SET created_at = '2000-01-01T00:00:00+00:00'",
                    [],
                )
                .unwrap();
        }
        assert_eq!(cache.cleanup_old(7), 1);
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn unavailable_cache_is_noop() {
        let cache = OfflineCache::open(Path::new("/nonexistent-dir/x/cache.db"));
        assert!(!cache.available());
        cache.enqueue("u", "b");
        assert!(cache.get_all().is_empty());
        assert!(cache.increment_attempts(1));
    }
}
