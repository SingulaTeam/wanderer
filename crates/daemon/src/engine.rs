//! The daemon's state machine: the guard, Xray, and the traffic count.
//!
//! See [`wanderer_tunnel::ipc`] for the states. The one rule every transition
//! keeps: the guard goes up before Xray starts and comes down only after Xray
//! has stopped, so no moment has a tunnel and no guard.

use std::path::PathBuf;

use wanderer_tunnel::ipc::{PROTOCOL_VERSION, Reply, State, Status, Traffic};
use wanderer_tunnel::xray::Host;
use wanderer_tunnel::{GuardPolicy, TunnelPlan};

use crate::guard;
use crate::xray::{self, Xray};

/// Everything the daemon controls.
pub(crate) struct Engine {
    xray_binary: PathBuf,
    host: Host,
    guard_up: bool,
    xray: Option<Xray>,
    /// Bytes from earlier Xray runs since the last connect from `Off`: each
    /// run gets a new interface, whose counters start at zero.
    earlier: Traffic,
    xray_exit: Option<String>,
    /// The connection that last asked for a tunnel or a hold: the app. When
    /// it goes away, so in time does the tunnel (see `server`).
    controller: Option<u64>,
}

impl Engine {
    pub(crate) fn new(xray_binary: PathBuf, host: Host) -> Self {
        Self {
            xray_binary,
            host,
            guard_up: false,
            xray: None,
            earlier: Traffic::default(),
            xray_exit: None,
            controller: None,
        }
    }

    /// The state, noticing first whether Xray has exited on its own.
    pub(crate) fn status(&mut self) -> Status {
        if let Some(exit) = self.xray.as_mut().and_then(Xray::exited) {
            tracing::warn!("{exit}; the guard holds");
            self.xray = None;
            self.xray_exit = Some(exit);
        }
        let state = match (self.guard_up, self.xray.is_some()) {
            (false, _) => State::Off,
            (true, true) => State::Up,
            (true, false) => State::Holding,
        };
        let now = if self.xray.is_some() {
            xray::traffic()
        } else {
            Traffic::default()
        };
        Status {
            version: PROTOCOL_VERSION,
            state,
            traffic: Traffic {
                up: self.earlier.up + now.up,
                down: self.earlier.down + now.down,
            },
            xray_exit: self.xray_exit.clone(),
        }
    }

    pub(crate) async fn connect(&mut self, plan: &TunnelPlan, policy: &GuardPolicy) -> Reply {
        if let Err(e) = plan.validate().and_then(|()| policy.validate()) {
            return self.error(e.to_string());
        }
        if !self.guard_up {
            self.earlier = Traffic::default();
        }
        // The new node is admitted before the old Xray stops; it loses its
        // node from the guard now, and that is fine: it is being replaced.
        if let Err(e) = guard::install(policy, Some(plan.node), false).await {
            return self.error(e);
        }
        self.guard_up = true;
        self.stop_xray().await;
        self.xray_exit = None;

        let config = wanderer_tunnel::xray::config(plan, self.host).to_string();
        match Xray::start(&self.xray_binary, &config).await {
            Ok(x) => {
                self.xray = Some(x);
                if guard::ADMITS_TUN_BY_INTERFACE {
                    if let Err(e) = guard::install(policy, Some(plan.node), true).await {
                        // The guard without the TUN holds everything back.
                        self.stop_xray().await;
                        return self.error(e);
                    }
                }
                tracing::info!(?plan, "connected");
                Reply::Ok(self.status())
            }
            Err(e) => {
                tracing::warn!("{e}; the guard holds");
                self.error(e.to_string())
            }
        }
    }

    pub(crate) async fn hold(&mut self, policy: &GuardPolicy) -> Reply {
        if let Err(e) = policy.validate() {
            return self.error(e.to_string());
        }
        if let Err(e) = guard::install(policy, None, false).await {
            return self.error(e);
        }
        self.guard_up = true;
        self.stop_xray().await;
        tracing::info!("holding");
        Reply::Ok(self.status())
    }

    pub(crate) async fn disconnect(&mut self) -> Reply {
        self.controller = None;
        self.stop_xray().await;
        if self.guard_up {
            if let Err(e) = guard::remove().await {
                // Still up: better offline than leaking.
                return self.error(e);
            }
            self.guard_up = false;
            tracing::info!("disconnected");
        }
        Reply::Ok(self.status())
    }

    /// Records `connection` as the one in control.
    pub(crate) fn claim(&mut self, connection: u64) {
        self.controller = Some(connection);
    }

    /// Whether `connection` is still the one in control, with something on.
    pub(crate) fn controlled_by(&mut self, connection: u64) -> bool {
        self.controller == Some(connection) && self.status().state != State::Off
    }

    async fn stop_xray(&mut self) {
        if let Some(x) = self.xray.take() {
            let last = xray::traffic();
            self.earlier.up += last.up;
            self.earlier.down += last.down;
            x.stop().await;
        }
    }

    fn error(&mut self, message: String) -> Reply {
        Reply::Error {
            message,
            status: self.status(),
        }
    }
}
