//! Running as a Windows service.
//!
//! The installer registers `wandererd` with the service manager, which starts
//! it at boot as `LocalSystem`. The manager calls in on a thread of its own;
//! the daemon runs on a Tokio runtime until the manager says stop, then takes
//! the tunnel and the guard down as it does on SIGTERM on Linux.

use std::ffi::OsString;
use std::time::Duration;

use tokio::sync::oneshot;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};

/// The service's name, as the installer registers it.
pub(crate) const NAME: &str = "wandererd";

define_windows_service!(ffi_service_main, service_main);

/// Hands the process to the service manager; returns when the service stops.
///
/// # Errors
///
/// Fails when the process was not started by the service manager.
pub(crate) fn run() -> Result<(), String> {
    service_dispatcher::start(NAME, ffi_service_main).map_err(|e| e.to_string())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = serve() {
        tracing::error!("{e}");
    }
}

fn serve() -> Result<(), String> {
    let (stop, stopped) = oneshot::channel::<()>();
    let mut stop = Some(stop);
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(stop) = stop.take() {
                let _ = stop.send(());
            }
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let status = service_control_handler::register(NAME, handler).map_err(|e| e.to_string())?;
    let report = |state, accepted| {
        status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })
    };
    report(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    )
    .map_err(|e| e.to_string())?;

    let outcome = tokio::runtime::Runtime::new()
        .map_err(|e| e.to_string())
        .and_then(|runtime| {
            runtime.block_on(crate::run(crate::args()?, async {
                let _ = stopped.await;
            }))
        });
    report(ServiceState::Stopped, ServiceControlAccept::empty()).map_err(|e| e.to_string())?;
    outcome
}
