//! Port of FileLogger.cs: `{DataDir}/logs/agent.log`, 5 MB rotation to `.1`/`.2` (rotated at startup
//! too), line format `yyyy-MM-ddTHH:mm:ssZ [INF] Category: message`. Also mirrors to stderr.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

struct Logger {
    dir: Option<PathBuf>,
    file: Mutex<Option<File>>,
    console: bool,
}

pub fn init(logs_dir: Option<&Path>, console: bool) {
    let dir = logs_dir.map(Path::to_path_buf);
    if let Some(d) = &dir {
        let _ = std::fs::create_dir_all(d);
    }
    let level = match std::env::var("ORGINV_LOG_LEVEL")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        "warn" => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let logger = Logger {
        dir,
        file: Mutex::new(None),
        console,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(level);
    }
}

fn level_tag(l: Level) -> &'static str {
    match l {
        Level::Trace => "TRC",
        Level::Debug => "DBG",
        Level::Info => "INF",
        Level::Warn => "WRN",
        Level::Error => "ERR",
    }
}

pub fn format_line(level: Level, target: &str, msg: &str) -> String {
    let cat = target.rsplit("::").next().unwrap_or(target);
    let cat = if cat.chars().count() > 48 {
        &cat[cat.len() - 48..]
    } else {
        cat
    };
    format!(
        "{} [{}] {}: {}",
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        level_tag(level),
        cat,
        msg
    )
}

impl Logger {
    fn write_file(&self, line: &str) {
        let Some(dir) = &self.dir else { return };
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        let path = dir.join("agent.log");
        if guard.is_some()
            && std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= MAX_FILE_BYTES
        {
            *guard = None;
        }
        if guard.is_none() {
            rotate(dir);
            *guard = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok();
        }
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn rotate(dir: &Path) {
    let f0 = dir.join("agent.log");
    let f1 = dir.join("agent.log.1");
    let f2 = dir.join("agent.log.2");
    let _ = std::fs::remove_file(&f2);
    let _ = std::fs::rename(&f1, &f2);
    let _ = std::fs::rename(&f0, &f1);
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        // Keep third-party crates (hyper, rustls, …) quiet below WARN.
        let ours = !r.target().contains("::")
            || r.target().starts_with("agent_core")
            || r.target().starts_with("collectors_linux")
            || r.target().starts_with("orginventory");
        if !ours && r.level() > Level::Warn {
            return;
        }
        let line = format_line(r.level(), r.target(), &r.args().to_string());
        if self.console {
            eprintln!("{line}");
        }
        self.write_file(&line);
    }

    fn flush(&self) {}
}
