//! Canonical JSON + SHA-256, port of CanonicalJson (.NET) / `json.dumps(sort_keys=True,
//! separators=(",", ":"), ensure_ascii=False)` (backend). Keys sorted ordinally, compact,
//! non-ASCII kept raw, integral floats written without ".0" (System.Text.Json style).

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub fn to_canonical_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

pub fn hash_value(value: &Value) -> String {
    hex::encode(Sha256::digest(to_canonical_string(value).as_bytes()))
}

/// Serialize `payload` (nulls already omitted by the DTOs), drop `exclude` at top level, hash.
pub fn hash<T: Serialize>(payload: &T, exclude: Option<&str>) -> String {
    let mut v = serde_json::to_value(payload).unwrap_or(Value::Null);
    if let (Some(key), Value::Object(map)) = (exclude, &mut v) {
        map.remove(key);
    }
    hash_value(&v)
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_value(out, &m[k]);
            }
            out.push('}');
        }
    }
}

fn write_number(out: &mut String, n: &serde_json::Number) {
    if n.is_i64() || n.is_u64() {
        let _ = write!(out, "{n}");
        return;
    }
    let f = n.as_f64().unwrap_or(0.0);
    if f.fract() == 0.0 && f.abs() < 1e15 {
        let _ = write!(out, "{}", f as i64);
    } else {
        let _ = write!(out, "{f}");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_keys_recursively_and_is_compact() {
        let v = json!({"b": 1, "a": {"d": [3, {"z": 1, "y": 2}], "c": "x"}});
        assert_eq!(
            to_canonical_string(&v),
            r#"{"a":{"c":"x","d":[3,{"y":2,"z":1}]},"b":1}"#
        );
    }

    #[test]
    fn keeps_unicode_and_integral_floats() {
        let v = json!({"name": "Công an", "size_gb": 477.0, "ram_gb": 15.6});
        assert_eq!(
            to_canonical_string(&v),
            r#"{"name":"Công an","ram_gb":15.6,"size_gb":477}"#
        );
    }

    #[test]
    fn excludes_property() {
        let a = json!({"x": 1, "config_hash": "abc"});
        let b = json!({"x": 1});
        assert_eq!(hash(&a, Some("config_hash")), hash_value(&b));
    }

    #[test]
    fn matches_python_reference_hash() {
        // python3 -c 'import json,hashlib;print(hashlib.sha256(json.dumps({"endpoints":["https://a"],"heartbeat_interval_seconds":30},sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()).hexdigest())'
        let v = json!({"heartbeat_interval_seconds": 30, "endpoints": ["https://a"]});
        assert_eq!(
            to_canonical_string(&v),
            r#"{"endpoints":["https://a"],"heartbeat_interval_seconds":30}"#
        );
    }
}
