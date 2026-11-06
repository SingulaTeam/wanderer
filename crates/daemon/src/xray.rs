//! Running Xray.
//!
//! Xray runs as a child of the daemon, with its configuration on stdin: it
//! never touches the disk, because it carries the session's credential. It
//! listens on nothing — other programs on the machine scan localhost for
//! proxies — and its traffic is counted on the TUN interface instead of
//! through its API.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::process::{Child, Command};
use wanderer_tunnel::TUN_NAME;
use wanderer_tunnel::ipc::Traffic;

/// How long Xray has to bring the TUN up.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// How long Xray has to undo its routes and DNS before it is killed.
#[cfg(unix)]
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
/// How much of Xray's log is kept to explain a failure.
const LOG_TAIL: usize = 20;

/// Why Xray did not start.
#[derive(Debug, thiserror::Error)]
pub(crate) enum StartError {
    /// The binary could not be run.
    #[error("cannot run Xray: {0}")]
    Spawn(#[from] std::io::Error),
    /// Xray exited, or did not bring the TUN up in time.
    #[error("Xray did not start: {0}")]
    Failed(String),
}

/// A running Xray.
pub(crate) struct Xray {
    child: Child,
    log: Arc<Mutex<VecDeque<String>>>,
}

impl Xray {
    /// Starts Xray on `config` and waits for its TUN interface.
    pub(crate) async fn start(binary: &Path, config: &str) -> Result<Self, StartError> {
        let mut command = Command::new(binary);
        command
            .args(["run", "-format", "json", "-c", "stdin:"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // No console window flashing up from a service.
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(config.as_bytes()).await?;
        }

        let log = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_TAIL)));
        if let Some(out) = child.stdout.take() {
            tokio::spawn(forward(out, Arc::clone(&log)));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(forward(err, Arc::clone(&log)));
        }

        let mut xray = Self { child, log };
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            if let Some(exit) = xray.exited() {
                return Err(StartError::Failed(exit));
            }
            if tun_exists() {
                return Ok(xray);
            }
            if tokio::time::Instant::now() >= deadline {
                let tail = xray.tail();
                xray.stop().await;
                return Err(StartError::Failed(format!("no TUN interface; {tail}")));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Why Xray stopped, if it has.
    pub(crate) fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(format!("Xray exited ({status}); {}", self.tail())),
            Err(e) => Some(format!("Xray is unreachable: {e}")),
        }
    }

    /// Stops Xray, letting it remove its routes and DNS settings first.
    #[cfg(unix)]
    pub(crate) async fn stop(mut self) {
        use rustix::process::{Pid, Signal, kill_process};
        if let Some(pid) = self
            .child
            .id()
            .and_then(|id| Pid::from_raw(id.cast_signed()))
        {
            let _ = kill_process(pid, Signal::TERM);
            if tokio::time::timeout(STOP_TIMEOUT, self.child.wait())
                .await
                .is_ok()
            {
                return;
            }
            tracing::warn!("Xray did not stop in time; killing it");
        }
        let _ = self.child.kill().await;
    }

    /// Stops Xray. A process without a console has no gentler signal on
    /// Windows; what Xray set up goes with it — the Wintun adapter, and with
    /// it the routes, the DNS servers and its own leak filters, which live
    /// in a session that ends with the process.
    #[cfg(windows)]
    pub(crate) async fn stop(mut self) {
        let _ = self.child.kill().await;
    }

    fn tail(&self) -> String {
        let log = self
            .log
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if log.is_empty() {
            "no output".to_owned()
        } else {
            log.iter().cloned().collect::<Vec<_>>().join(" | ")
        }
    }
}

/// Where Xray is installed, unless `--xray` says otherwise.
#[cfg(unix)]
pub(crate) fn default_path() -> PathBuf {
    "/usr/lib/wanderer/xray".into()
}

/// Where Xray is installed, unless `--xray` says otherwise: beside the
/// daemon, with `wintun.dll`.
#[cfg(windows)]
pub(crate) fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("xray.exe")))
        .unwrap_or_else(|| "xray.exe".into())
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(unix)]
fn tun_exists() -> bool {
    interface_dir().exists()
}

#[cfg(windows)]
fn tun_exists() -> bool {
    crate::windows::iface::luid(TUN_NAME).is_some()
}

/// The bytes on the TUN interface now; zero when it does not exist.
///
/// For a TUN, what the machine sends into the tunnel is the interface's
/// transmit side.
#[cfg(windows)]
pub(crate) fn traffic() -> Traffic {
    let (up, down) = crate::windows::iface::octets(TUN_NAME).unwrap_or((0, 0));
    Traffic { up, down }
}

/// The bytes on the TUN interface now; zero when it does not exist.
///
/// For a TUN, what the machine sends into the tunnel is the interface's
/// transmit side.
#[cfg(unix)]
pub(crate) fn traffic() -> Traffic {
    let read = |name: &str| -> u64 {
        std::fs::read_to_string(interface_dir().join("statistics").join(name))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    };
    Traffic {
        up: read("tx_bytes"),
        down: read("rx_bytes"),
    }
}

#[cfg(unix)]
fn interface_dir() -> PathBuf {
    Path::new("/sys/class/net").join(TUN_NAME)
}

/// Passes Xray's output to the daemon's log and keeps its last lines.
async fn forward(stream: impl tokio::io::AsyncRead + Unpin, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!(target: "xray", "{line}");
        let mut tail = tail
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if tail.len() == LOG_TAIL {
            tail.pop_front();
        }
        tail.push_back(line);
    }
}
