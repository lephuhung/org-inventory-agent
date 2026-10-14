//! Port of MacosSoftwareCollector.cs: SPApplicationsDataType, fallback *.app scan. Cap 500.

use crate::cmd;
use agent_core::schema::SoftwareInfo;
use serde_json::Value;

const MAX_APPS: usize = 500;

pub fn collect() -> Vec<SoftwareInfo> {
    if let Some(doc) = cmd::system_profiler_json("SPApplicationsDataType", 60_000) {
        let list = parse_applications_data_type(&doc);
        if !list.is_empty() {
            return list;
        }
    }
    log::debug!(target: "MacosSoftwareCollector", "SPApplicationsDataType lỗi/rỗng — fallback scan /Applications.");
    scan_applications_dirs()
}

pub fn parse_applications_data_type(root: &Value) -> Vec<SoftwareInfo> {
    let Some(items) = root.get("SPApplicationsDataType").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|app| {
            let name = cmd::json_string(app, &["_name"])?;
            let path = cmd::json_string(app, &["path"]);
            Some(SoftwareInfo {
                display_name: Some(name.clone()),
                name: Some(name),
                version: cmd::json_string(app, &["version"]),
                publisher: map_obtained_from(cmd::json_string(app, &["obtained_from"])),
                is_per_user: path.is_some_and(|p| p.starts_with("/Users/")),
                ..Default::default()
            })
        })
        .take(MAX_APPS)
        .collect()
}

fn map_obtained_from(v: Option<String>) -> Option<String> {
    match v.as_deref() {
        Some("apple") => Some("Apple".into()),
        Some("mac_app_store") => Some("Mac App Store".into()),
        Some("identified_developer") => Some("Identified Developer".into()),
        Some("unknown") => None,
        _ => v,
    }
}

fn scan_applications_dirs() -> Vec<SoftwareInfo> {
    let mut list = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for dir in [
        "/Applications",
        "/System/Applications",
        "/System/Applications/Utilities",
    ] {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix(".app"))
                    .map(str::to_string)
            })
            .collect();
        names.sort();
        for name in names {
            if seen.insert(name.to_lowercase()) {
                list.push(SoftwareInfo {
                    display_name: Some(name.clone()),
                    name: Some(name),
                    ..Default::default()
                });
                if list.len() >= MAX_APPS {
                    return list;
                }
            }
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_fields() {
        let v = json!({"SPApplicationsDataType":[
          {"_name":"Safari","path":"/Applications/Safari.app","version":"17.5","obtained_from":"apple"},
          {"_name":"CustomApp","path":"/Users/me/Applications/CustomApp.app","version":"1.2","obtained_from":"identified_developer"},
          {"path":"/Applications/NoName.app"}
        ]});
        let apps = parse_applications_data_type(&v);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].display_name.as_deref(), Some("Safari"));
        assert_eq!(apps[0].version.as_deref(), Some("17.5"));
        assert_eq!(apps[0].publisher.as_deref(), Some("Apple"));
        assert!(!apps[0].is_per_user);
        assert!(apps[1].is_per_user);
        assert_eq!(apps[1].publisher.as_deref(), Some("Identified Developer"));
    }

    #[test]
    fn caps_at_500() {
        let items: Vec<_> = (0..600)
            .map(|i| json!({"_name": format!("App{i}")}))
            .collect();
        assert_eq!(
            parse_applications_data_type(&json!({"SPApplicationsDataType": items})).len(),
            500
        );
    }
}
