//! OrgInventory Agent — Linux entry point (port of OrgInventoryAgent.Linux/Program.cs).

use agent_core::api::ApiClient;
use agent_core::cli::CliArgs;
use agent_core::config::{AgentConfig, PosixConfig, DEFAULT_BOOTSTRAP_PATH};
use agent_core::endpoint::EndpointManager;
use agent_core::keystore::KeyStore;
use agent_core::services::{
    cache::OfflineCache, config_sync, enroll::EnrollCoordinator, heartbeat::HeartbeatService,
    inventory::InventoryService, renew::RenewService, update::UpdateService,
};
use agent_core::state::AgentState;
use agent_core::{identity, paths, SharedConfig, SharedState, APP_NAME, VERSION};
use collectors_linux::LinuxInventoryProvider;
use serde_json::json;
use std::sync::{Arc, Mutex};

const ABOUT: &str = include_str!("about.txt");

fn help() -> String {
    format!(
        "OrgInventoryAgent v{VERSION} — Linux Agent
Sử dụng: OrgInventoryAgent [tùy chọn]
  --data-dir <path>        Thư mục dữ liệu (config/cache/log)
  --config <path>          File config.json (mặc định /etc/orginventory/config.json)
  --enroll-token <token>   Enroll token (lưu vào config, xóa sau enroll)
  --endpoint <url>         Server URL mTLS
  --inventory-seconds <n>  Chu kỳ inventory (giây, test)
  --once                   Enroll (nếu cần) → inventory 1 lần rồi exit
  --send-inventory         Gửi inventory 1 lần rồi exit (tương đương --once)
  --check-update           Kiểm tra phiên bản mới trên release (không tải) rồi exit
  --update-now             Kiểm tra, tải và áp dụng bản mới ngay rồi exit
  --print-config           In cấu hình hiện tại
  --print-inventory        In payload inventory v4 đầy đủ
  --print-security         In security posture
  --print-fingerprint      In fingerprint
  --version, -v            In version
  --about, --info          In cam kết an toàn
  --help, -h               In hướng dẫn này"
    )
}

fn pretty<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn mask_token(t: &Option<String>) -> Option<String> {
    t.as_ref().map(|t| {
        if t.chars().count() > 12 {
            format!("{}…", t.chars().take(4).collect::<String>())
        } else {
            "***".into()
        }
    })
}

struct Runtime {
    config: SharedConfig,
    state: SharedState,
    api: Arc<ApiClient>,
    keystore: KeyStore,
    enroll: Arc<EnrollCoordinator>,
    cache: Arc<OfflineCache>,
    inventory: Arc<InventoryService>,
}

fn build_runtime(config: AgentConfig) -> Runtime {
    let config: SharedConfig = Arc::new(Mutex::new(config));
    let state: SharedState = Arc::new(Mutex::new(AgentState::load()));
    let keystore = KeyStore::in_data_dir();
    let endpoints = Arc::new(EndpointManager::new(config.clone()));
    let api = Arc::new(ApiClient::new(config.clone(), endpoints, keystore.clone()));
    let enroll = Arc::new(EnrollCoordinator::new(
        config.clone(),
        api.clone(),
        keystore.clone(),
        state.clone(),
        Arc::new(collectors_linux::fingerprint::collect),
    ));
    let cache = Arc::new(OfflineCache::open(&paths::cache_db_file()));
    let inventory = Arc::new(InventoryService::new(
        config.clone(),
        api.clone(),
        enroll.clone(),
        cache.clone(),
        Arc::new(LinuxInventoryProvider),
        state.clone(),
    ));
    Runtime {
        config,
        state,
        api,
        keystore,
        enroll,
        cache,
        inventory,
    }
}

impl Runtime {
    fn heartbeat(&self) -> Arc<HeartbeatService> {
        Arc::new(HeartbeatService {
            config: self.config.clone(),
            api: self.api.clone(),
            enroll: self.enroll.clone(),
            cache: self.cache.clone(),
            inventory: self.inventory.clone(),
            keystore: self.keystore.clone(),
            state: self.state.clone(),
        })
    }
}

#[tokio::main]
async fn main() {
    std::process::exit(run().await);
}

async fn run() -> i32 {
    let cli = CliArgs::parse(std::env::args().skip(1));
    let data_dir = paths::resolve_data_dir(cli.data_dir.as_deref());
    if let Err(e) = paths::initialize(data_dir.clone()) {
        eprintln!("Không thể khởi tạo data dir '{}': {e}", data_dir.display());
        return 10;
    }
    if cli.print_version {
        println!("{APP_NAME} v{VERSION}");
        return 0;
    }
    if cli.show_help {
        println!("{}", help());
        return 0;
    }
    if cli.print_about {
        println!("{ABOUT}");
        return 0;
    }
    let posix = PosixConfig::new(cli.config_path.as_deref(), paths::config_file());
    if cli.print_config {
        let cfg = posix.load();
        println!(
            "{}",
            pretty(&json!({
                "data_dir": paths::data_dir(),
                "config_path": cli.config_path.clone().unwrap_or_else(|| DEFAULT_BOOTSTRAP_PATH.into()),
                "endpoints": cfg.endpoints,
                "heartbeat_interval_seconds": cfg.heartbeat_interval_seconds,
                "heartbeat_jitter_seconds": cfg.heartbeat_jitter_seconds,
                "inventory_interval_hours": cfg.inventory_interval_hours,
                "renew_before_percent": cfg.renew_before_percent,
                "enrolled": cfg.enrolled,
                "machine_id": cfg.machine_id,
                "token": mask_token(&cfg.token),
                "client_cert_thumbprint": cfg.client_cert_thumbprint,
                "cert_store_location": cfg.cert_store_location,
            }))
        );
        return 0;
    }
    if cli.print_inventory || cli.print_security || cli.print_fingerprint {
        agent_core::logging::init(None, true);
        use agent_core::payload::InventoryProvider;
        if cli.print_inventory {
            let p = LinuxInventoryProvider;
            println!(
                "{}",
                pretty(&json!({ "envelope": p.envelope(), "snapshot": p.snapshot() }))
            );
        } else if cli.print_security {
            println!("{}", pretty(&collectors_linux::security::collect()));
        } else {
            println!("{}", pretty(&collectors_linux::fingerprint::collect()));
        }
        return 0;
    }

    let mut config = posix.load();
    if let Some(ep) = cli.endpoint.as_deref().filter(|s| !s.trim().is_empty()) {
        config.set_primary_endpoint(ep);
    }
    if let Some(s) = cli.inventory_seconds {
        config.inventory_interval_seconds = Some(s);
    }
    if let Some(t) = cli.enroll_token.as_deref().filter(|s| !s.trim().is_empty()) {
        config.token = Some(t.to_string());
        if let Err(e) = posix.save(&mut config) {
            eprintln!("Không lưu được config: {e}");
        }
    }

    agent_core::logging::init(Some(&paths::logs_dir()), true);

    if cli.check_update || cli.update_now {
        return run_update(config, cli.update_now).await;
    }
    if cli.once || cli.send_inventory {
        return run_once(config).await;
    }
    run_service(config, cli.config_path).await
}

async fn run_update(config: AgentConfig, apply: bool) -> i32 {
    let config: SharedConfig = Arc::new(Mutex::new(config));
    let state: SharedState = Arc::new(Mutex::new(AgentState::load()));
    let svc = match UpdateService::new(config, state) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Không khởi tạo được UpdateService: {e}");
            return 1;
        }
    };
    let fut = async {
        if !apply {
            let r = svc.check().await;
            println!(
                "{}",
                pretty(&json!({
                    "current_version": r.current_version,
                    "latest_version": r.latest_version,
                    "update_available": r.update_available,
                    "pending_version": r.pending_version,
                    "error": r.error,
                }))
            );
            return if r.error.is_none() { 0 } else { 1 };
        }
        let staged = svc.check_and_apply().await;
        println!(
            "{}",
            if staged {
                "Đã tải và áp dụng/stage bản mới (xem log)."
            } else {
                "Không có bản mới hoặc cập nhật thất bại (xem log)."
            }
        );
        if staged {
            0
        } else {
            2
        }
    };
    match tokio::time::timeout(std::time::Duration::from_secs(12 * 60), fut).await {
        Ok(code) => code,
        Err(_) => {
            eprintln!("Hết thời gian kiểm tra cập nhật.");
            1
        }
    }
}

async fn run_once(config: AgentConfig) -> i32 {
    log::info!(target: "Once", "Chế độ --once: enroll (nếu cần) → heartbeat → inventory.");
    let rt = build_runtime(config);
    let ok = rt.enroll.ensure_enrolled().await;
    if !ok && !identity::is_enrolled(&rt.config.lock().unwrap()) {
        log::warn!(target: "Once", "Chưa enroll được — exit 0 (service sẽ retry).");
        return 0;
    }
    let hb_ok = rt.heartbeat().send_once().await;
    log::info!(target: "Once", "--once heartbeat: {}", if hb_ok { "OK" } else { "FAIL" });
    rt.inventory.send_once().await;
    0
}

async fn run_service(config: AgentConfig, config_path: Option<String>) -> i32 {
    let rule = "=".repeat(80);
    log::info!(target: "Startup", "{rule}");
    log::info!(target: "Startup", "{APP_NAME} v{VERSION} — {}", agent_core::FULL_TITLE);
    log::info!(target: "Startup", "Đơn vị phát triển: {}", agent_core::DEVELOPER_SHORT);
    log::info!(
        target: "Startup",
        "DataDir={} Config={}",
        paths::data_dir().display(),
        config_path.unwrap_or_else(|| DEFAULT_BOOTSTRAP_PATH.into())
    );
    log::info!(target: "Startup", "{rule}");

    let rt = build_runtime(config);
    let update = UpdateService::new(rt.config.clone(), rt.state.clone());
    let renew = Arc::new(RenewService {
        config: rt.config.clone(),
        api: rt.api.clone(),
        enroll: rt.enroll.clone(),
        keystore: rt.keystore.clone(),
    });

    let enroll = rt.enroll.clone();
    tokio::spawn(async move {
        enroll.ensure_enrolled().await;
    });
    tokio::spawn(rt.heartbeat().run());
    tokio::spawn(rt.inventory.clone().run());
    tokio::spawn(renew.run());
    tokio::spawn(config_sync::run(
        rt.config.clone(),
        rt.api.clone(),
        rt.enroll.clone(),
    ));
    match update {
        Ok(u) => {
            tokio::spawn(u.run());
        }
        Err(e) => log::warn!(target: "Startup", "Không khởi tạo được UpdateService: {e}"),
    }

    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
            return 0;
        }
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    log::info!(target: "Startup", "Nhận tín hiệu dừng — thoát.");
    0
}
