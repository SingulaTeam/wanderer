//! The tunnel on Android: the app's own VPN service in place of the daemon.
//!
//! Android has no privileged service to talk to and no way to run Xray as a
//! program, so Xray runs inside the app (`android/xray`, bound into an AAR)
//! and the VPN service owns the interface (`TunnelService.kt`). This is the
//! [`Engine`] the supervisor drives, over the Tauri plugin `TunnelPlugin.kt`;
//! the states and the status are the daemon's.
//!
//! There is no privilege boundary here, so the plugin takes the Xray
//! configuration itself, built from the same plan as on a desktop.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::Wry;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use wanderer_core::daemon::DaemonError;
use wanderer_core::engine::Engine;
use wanderer_tunnel::ipc::{PROTOCOL_VERSION, State, Status, Traffic};
use wanderer_tunnel::xray::Host;
use wanderer_tunnel::{GuardPolicy, TunnelPlan};

const IDENTIFIER: &str = "network.wanderer.app";
const CLASS: &str = "TunnelPlugin";

static PLUGIN: OnceLock<PluginHandle<Wry>> = OnceLock::new();

/// Registers the plugin; add it to the builder.
pub(crate) fn init() -> TauriPlugin<Wry> {
    Builder::new("tunnel")
        .setup(|_, api| {
            let handle = api.register_android_plugin(IDENTIFIER, CLASS)?;
            let _ = PLUGIN.set(handle);
            Ok(())
        })
        .build()
}

/// The VPN service, as the supervisor's [`Engine`].
#[derive(Default)]
pub(crate) struct VpnService;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectArgs {
    config: String,
    /// The node, "ip:port": the tunnel's sockets to it bypass the VPN.
    node: String,
    allow_lan: bool,
}

#[derive(Serialize)]
struct RelayArgs<'a> {
    config: &'a str,
}

/// Runs the relay to Control Planes' fronts (see `relay`).
pub(crate) async fn start_relay(config: &str) -> Result<(), String> {
    call::<serde_json::Value>("startRelay", RelayArgs { config })
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HoldArgs {
    allow_lan: bool,
}

#[derive(Deserialize)]
struct Prepared {
    granted: bool,
}

/// The plugin's answer: the status, and the failure if there was one.
#[derive(Deserialize)]
struct Answer {
    status: WireStatus,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct WireStatus {
    state: String,
    up: u64,
    down: u64,
    #[serde(default)]
    exit: Option<String>,
}

impl From<WireStatus> for Status {
    fn from(s: WireStatus) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            state: match s.state.as_str() {
                "up" => State::Up,
                "holding" => State::Holding,
                _ => State::Off,
            },
            traffic: Traffic {
                up: s.up,
                down: s.down,
            },
            xray_exit: s.exit,
        }
    }
}

async fn call<T: for<'de> Deserialize<'de>>(
    command: &str,
    args: impl Serialize,
) -> Result<T, DaemonError> {
    let plugin = PLUGIN.get().ok_or_else(|| {
        DaemonError::Unreachable(std::io::Error::other("the tunnel plugin is not registered"))
    })?;
    plugin
        .run_mobile_plugin_async(command, args)
        .await
        .map_err(|e| DaemonError::Unreachable(std::io::Error::other(e.to_string())))
}

async fn request(command: &str, args: impl Serialize) -> Result<Status, DaemonError> {
    let answer: Answer = call(command, args).await?;
    let status = Status::from(answer.status);
    match answer.error {
        None => Ok(status),
        Some(message) => Err(DaemonError::Refused { message, status }),
    }
}

/// Asks for the VPN permission if the app does not hold it yet.
async fn prepare() -> Result<(), DaemonError> {
    let prepared: Prepared = call("prepare", ()).await?;
    if prepared.granted {
        Ok(())
    } else {
        Err(DaemonError::Refused {
            message: "the VPN permission was not granted".into(),
            status: off(),
        })
    }
}

fn off() -> Status {
    Status {
        version: PROTOCOL_VERSION,
        state: State::Off,
        traffic: Traffic::default(),
        xray_exit: None,
    }
}

impl Engine for VpnService {
    async fn status(&mut self) -> Result<Status, DaemonError> {
        request("status", ()).await
    }

    async fn connect(
        &mut self,
        plan: &TunnelPlan,
        guard: &GuardPolicy,
    ) -> Result<Status, DaemonError> {
        plan.validate().map_err(|e| DaemonError::Refused {
            message: e.to_string(),
            status: off(),
        })?;
        prepare().await?;
        let config = wanderer_tunnel::xray::config(plan, Host::Android).to_string();
        request(
            "connect",
            ConnectArgs {
                config,
                node: plan.node.to_string(),
                allow_lan: guard.allow_lan,
            },
        )
        .await
    }

    async fn hold(&mut self, guard: &GuardPolicy) -> Result<Status, DaemonError> {
        prepare().await?;
        request(
            "hold",
            HoldArgs {
                allow_lan: guard.allow_lan,
            },
        )
        .await
    }

    async fn disconnect(&mut self) -> Result<Status, DaemonError> {
        request("disconnect", ()).await
    }

    /// The app is outside the interface while it holds and inside the tunnel
    /// while it is up, so the system resolver works either way.
    async fn resolve(&mut self, host: &str) -> Result<Vec<Ipv4Addr>, DaemonError> {
        let addresses = tokio::net::lookup_host((host, 443))
            .await
            .map_err(DaemonError::Unreachable)?
            .filter_map(|a| match a.ip() {
                IpAddr::V4(ip) => Some(ip),
                IpAddr::V6(_) => None,
            })
            .collect();
        Ok(addresses)
    }
}
