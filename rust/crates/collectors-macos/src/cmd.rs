//! Port of MacosCmd.cs: run system tools with a timeout; never fails, missing data → None.

use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// stdout of `program args` (None on error/timeout/blank output). stderr is discarded.
pub fn run(program: &str, args: &[&str], timeout_ms: u64) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = String::from_utf8_lossy(&reader.join().ok()?).into_owned();
    (!out.trim().is_empty()).then_some(out)
}

/// `sysctl -n <name>`, trimmed.
pub fn sysctl(name: &str) -> Option<String> {
    run("/usr/sbin/sysctl", &["-n", name], 5_000).map(|s| s.trim().to_string())
}

/// `system_profiler <dataType> -json`.
pub fn system_profiler_json(data_type: &str, timeout_ms: u64) -> Option<Value> {
    let json = run(
        "/usr/sbin/system_profiler",
        &[data_type, "-json"],
        timeout_ms,
    )?;
    serde_json::from_str(&json).ok()
}

/// First non-blank string value among `keys` of a JSON object.
pub fn json_string(el: &Value, keys: &[&str]) -> Option<String> {
    let obj = el.as_object()?;
    keys.iter().find_map(|k| {
        obj.get(*k)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    })
}

/// First element of the `<dataType>` array.
pub fn first_item<'a>(root: &'a Value, data_type: &str) -> Option<&'a Value> {
    root.get(data_type)?.as_array()?.first()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_string_skips_blank_and_non_string() {
        let v: Value = serde_json::json!({"a": " ", "b": 3, "c": "x"});
        assert_eq!(json_string(&v, &["a", "b", "c"]).as_deref(), Some("x"));
        assert_eq!(json_string(&v, &["z"]), None);
        assert_eq!(json_string(&Value::Null, &["a"]), None);
    }
}
