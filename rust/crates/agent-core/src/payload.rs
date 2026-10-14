//! Port of InventoryPayloadBuilder.cs — flat snapshot + v4 envelope + `config_hash`
//! (canonical hash of the payload without `config_hash`).

use crate::canonical;
use crate::schema::{InventoryEnvelope, InventorySnapshot};
use serde_json::{json, Map, Value};

pub trait InventoryProvider: Send + Sync {
    fn envelope(&self) -> InventoryEnvelope;
    fn snapshot(&self) -> InventorySnapshot;
}

fn put<T: serde::Serialize>(m: &mut Map<String, Value>, k: &str, v: &Option<T>) {
    if let Some(v) = v {
        m.insert(k.into(), serde_json::to_value(v).unwrap_or(Value::Null));
    }
}

pub fn build_from(envelope: &InventoryEnvelope, s: &InventorySnapshot, logged_user: &str) -> Value {
    let mut m = Map::new();
    put(&mut m, "os_name", &s.os_name);
    put(&mut m, "os_version", &s.os_version);
    put(&mut m, "os_build", &s.os_build);
    put(&mut m, "os_arch", &s.os_arch);
    put(&mut m, "is_vm", &s.is_vm);
    m.insert("logged_user".into(), json!(logged_user));
    put(&mut m, "cpu", &s.cpu);
    put(&mut m, "ram_gb", &s.ram_gb);
    put(&mut m, "disks", &s.disks);
    put(&mut m, "gpu", &s.gpu);
    put(&mut m, "mainboard", &s.mainboard);
    put(&mut m, "bios", &s.bios);
    put(&mut m, "network", &s.network);
    put(&mut m, "installed_software", &s.installed_software);
    m.insert(
        "security".into(),
        serde_json::to_value(&envelope.security).unwrap_or(Value::Null),
    );
    m.insert(
        "agent".into(),
        serde_json::to_value(&envelope.agent).unwrap_or(Value::Null),
    );
    m.insert(
        "os".into(),
        serde_json::to_value(&envelope.os).unwrap_or(Value::Null),
    );
    m.insert("inventory_schema_version".into(), json!(4));
    let mut v = Value::Object(m);
    let hash = canonical::hash(&v, Some("config_hash"));
    v.as_object_mut()
        .unwrap()
        .insert("config_hash".into(), json!(hash));
    v
}

pub fn build(provider: &dyn InventoryProvider, logged_user: &str) -> Value {
    build_from(&provider.envelope(), &provider.snapshot(), logged_user)
}
