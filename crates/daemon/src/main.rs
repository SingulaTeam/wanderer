//! `wandererd`, Wanderer's privileged half.
//!
//! Runs as root on Linux, started by systemd, and as a `LocalSystem` service on
//! Windows, and does only what needs those rights: Xray with a TUN
//! interface, and the Network Guard. The app tells it what to do over a local
//! socket — a Unix socket, or a named pipe on Windows; see
//! [`wanderer_tunnel::ipc`].
//!
//! ```text
//! wandererd [--socket PATH] [--xray PATH] [--app-grace SECONDS] [--console]
//! ```
//!
//! `--app-grace` is how long the tunnel outlives the app that asked for it
//! (60 seconds; 0 keeps it up regardless, for tests that drive the daemon
//! one short connection at a time). On Windows the daemon expects the
//! service manager to start it; `--console` runs it in a terminal instead.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use wanderer_tunnel::ipc::SOCKET_PATH;
use wanderer_tunnel::xray::Host;

mod engine;
mod guard;
#[cfg(unix)]
mod nft;
mod resolve;
mod server;
#[cfg(windows)]
mod windows;
mod xray;

struct Args {
    socket: PathBuf,
    xray: PathBuf,
    app_grace: Option<Duration>,
    console: bool,
}

/// How long the tunnel waits for an app that went away: long enough for it
/// to restart, short of the minutes in which its session would be swept.
const APP_GRACE: Duration = Duration::from_secs(60);

fn args() -> Result<Args, String> {
    let mut args = Args {
        socket: SOCKET_PATH.into(),
        xray: xray::default_path(),
        app_grace: Some(APP_GRACE),
        console: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        if flag == "--console" {
            args.console = true;
            continue;
        }
        let value = it.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--socket" => args.socket = value.into(),
            "--xray" => args.xray = value.into(),
            "--app-grace" => {
                let secs: u64 = value.parse().map_err(|_| "--app-grace takes seconds")?;
                args.app_grace = (secs > 0).then(|| Duration::from_secs(secs));
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    logging();
    let outcome = match args() {
        #[cfg(windows)]
        Ok(args) if !args.console => windows::service::run(),
        Ok(args) => tokio::runtime::Runtime::new()
            .map_err(|e| e.to_string())
            .and_then(|runtime| runtime.block_on(run(args, stop_signal()))),
        Err(e) => Err(e),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the daemon until `stop` completes, then takes the tunnel and the
/// guard down: stopping the daemon must not leave the machine offline
/// behind a guard nothing will ever remove.
async fn run(args: Args, stop: impl Future<Output = ()>) -> Result<(), String> {
    #[cfg(unix)]
    preflight()?;
    // A daemon that died while connected left its guard behind; the app finds
    // the daemon off and reconnects from there.
    guard::remove().await?;

    let host = host();
    tracing::info!(?host, xray = %args.xray.display(), "starting");
    let engine = Arc::new(Mutex::new(engine::Engine::new(args.xray, host)));
    tokio::select! {
        served = server::serve(&args.socket, Arc::clone(&engine), args.app_grace) => served?,
        () = stop => {}
    }
    tracing::info!("stopping");
    let _ = engine.lock().await.disconnect().await;
    server::cleanup(&args.socket);
    Ok(())
}

#[cfg(unix)]
fn preflight() -> Result<(), String> {
    if rustix::process::geteuid().is_root() {
        Ok(())
    } else {
        Err("wandererd must run as root".into())
    }
}

#[cfg(unix)]
fn host() -> Host {
    /// systemd-resolved's own socket, present while it runs.
    const RESOLVED: &str = "/run/systemd/resolve/io.systemd.Resolve";
    Host::Linux {
        systemd_resolved: std::path::Path::new(RESOLVED).exists(),
    }
}

#[cfg(windows)]
fn host() -> Host {
    Host::Windows
}

#[cfg(unix)]
async fn stop_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[cfg(windows)]
async fn stop_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// To the journal on Linux, which stamps every line. On Windows to a file in
/// the program data directory, truncated at each start; Xray's own lines are
/// left out there, as warnings can name the sites someone visited.
#[cfg(unix)]
fn logging() {
    use std::io::IsTerminal as _;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .without_time()
        .with_ansi(std::io::stdout().is_terminal())
        .init();
}

#[cfg(windows)]
fn logging() {
    let dir = std::env::var_os("ProgramData")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Wanderer");
    let file = std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::File::create(dir.join("wandererd.log")));
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,xray=off".into())
    };
    match file {
        Ok(file) => tracing_subscriber::fmt()
            .with_env_filter(filter())
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        Err(_) => tracing_subscriber::fmt().with_env_filter(filter()).init(),
    }
}
