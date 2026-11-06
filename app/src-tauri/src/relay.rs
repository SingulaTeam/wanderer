//! The relay to Control Planes' fronts.
//!
//! An Xray with one local entry per front (see `wanderer_core::front`). On a
//! desktop it runs as the app's child, with the app's rights — the Xray the
//! package installs for the daemon; on Android it is a second instance in the
//! app's process, beside the tunnel's.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};

use singula::StarId;
use wanderer_core::front::{Entry, relay_config};

/// A running relay, stopped when dropped.
pub(crate) struct Relay {
    entries: HashMap<StarId, u16>,
    #[cfg(desktop)]
    _child: tokio::process::Child,
}

impl Relay {
    /// Starts a relay to `fronts`: each Control Plane, the address of its
    /// front, and its REALITY key.
    pub(crate) async fn start(fronts: &[(StarId, SocketAddrV4, String)]) -> Result<Self, String> {
        let mut entries = Vec::new();
        for (star, front, public_key) in fronts {
            entries.push(Entry {
                star: *star,
                port: free_port()?,
                front: *front,
                public_key: public_key.clone(),
            });
        }
        let config = relay_config(&entries).to_string();
        let ports = entries.iter().map(|e| (e.star, e.port)).collect();
        Self::run(&config, ports).await
    }

    /// The local port of `star`'s entry, if it has a front.
    pub(crate) fn entry(&self, star: StarId) -> Option<u16> {
        self.entries.get(&star).copied()
    }

    #[cfg(desktop)]
    async fn run(config: &str, entries: HashMap<StarId, u16>) -> Result<Self, String> {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt as _;

        let mut command = tokio::process::Command::new(xray_path());
        command
            .args(["run", "-format", "json", "-c", "stdin:"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000); // no console window
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot run Xray for the relay: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(config.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
        }
        // Up once its entries take connections.
        let first = entries.values().next().copied();
        for _ in 0..50 {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the relay's Xray exited ({status})"));
            }
            match first {
                Some(port)
                    if tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                        .await
                        .is_err() =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                _ => break,
            }
        }
        Ok(Self {
            entries,
            _child: child,
        })
    }

    #[cfg(target_os = "android")]
    async fn run(config: &str, entries: HashMap<StarId, u16>) -> Result<Self, String> {
        crate::android::start_relay(config).await?;
        Ok(Self { entries })
    }
}

/// A port on 127.0.0.1 nothing listens on now.
fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| format!("no free local port: {e}"))
}

/// The Xray the package installs; `WANDERER_XRAY` overrides it for
/// development.
#[cfg(desktop)]
fn xray_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("WANDERER_XRAY") {
        return path.into();
    }
    #[cfg(windows)]
    {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|dir| dir.join("xray.exe")))
            .unwrap_or_else(|| "xray.exe".into())
    }
    #[cfg(not(windows))]
    {
        "/usr/lib/wanderer/xray".into()
    }
}
