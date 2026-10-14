//! Windows service support — port of `AddWindowsService("OrgInventoryAgent")`.
//! SCM gọi `service_main` trên thread riêng; STOP/SHUTDOWN báo qua watch channel
//! mà `shutdown_signal()` trong main.rs lắng nghe.

use std::time::Duration;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

windows_service::define_windows_service!(ffi_service_main, service_main);

/// Được gọi từ `main` trước khi tokio runtime chạy. Ok = đã chạy service xong;
/// Err(1063) = không phải SCM (console) → fall back chạy console.
pub fn dispatch() -> Result<(), windows_service::Error> {
    service_dispatcher::start("OrgInventoryAgent", ffi_service_main)
}

fn status(state: ServiceState) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(30),
        process_id: None,
    }
}

fn service_main(_args: Vec<std::ffi::OsString>) {
    let (tx, _rx) = tokio::sync::watch::channel(false);
    let _ = crate::STOP_TX.set(tx);
    let tx = match crate::STOP_TX.get() {
        Some(t) => t.clone(),
        None => return,
    };
    let handler =
        match service_control_handler::register("OrgInventoryAgent", move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(true);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("Không đăng ký được service control handler: {e}");
                return;
            }
        };
    let _ = handler.set_service_status(status(ServiceState::Running));
    match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(crate::run()),
        Err(e) => {
            eprintln!("Không tạo được tokio runtime: {e}");
            1
        }
    };
    let _ = handler.set_service_status(status(ServiceState::Stopped));
}
