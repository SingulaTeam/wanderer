//! The requests and replies on the daemon's socket.
//!
//! One JSON object per line each way, a reply for every request, in order.
//! The app drives the daemon through three states:
//!
//! ```text
//!            Connect             Hold
//!   Off ───────────────▶ Up ─────────────▶ Holding
//!    ▲                   ▲ ◀───────────────┘ │
//!    │                   │      Connect      │
//!    └───────────────────┴───────────────────┘
//!                     Disconnect
//! ```
//!
//! The guard is up in `Up` and in `Holding`; Xray runs only in `Up`. If Xray
//! exits on its own, the daemon moves to `Holding` and says why in the next
//! [`Status`]: the guard keeps holding until the app reconnects or
//! disconnects, so a crash never becomes a leak.

use serde::{Deserialize, Serialize};

use crate::plan::{GuardPolicy, TunnelPlan};

/// Bumped on any change the other side cannot read.
pub const PROTOCOL_VERSION: u32 = 3;

/// Where the daemon listens: a Unix socket on Linux, a named pipe on
/// Windows.
#[cfg(unix)]
pub const SOCKET_PATH: &str = "/run/wanderer/daemon.sock";
/// Where the daemon listens: a Unix socket on Linux, a named pipe on
/// Windows.
#[cfg(windows)]
pub const SOCKET_PATH: &str = r"\\.\pipe\wanderer-daemon";

/// The longest line either side accepts.
pub const MAX_LINE: usize = 64 * 1024;

/// What the app asks of the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "request")]
pub enum Request {
    /// Report the state, and the daemon's protocol version.
    Status,
    /// Put up the guard for `guard` and the plan's node, then start Xray on
    /// `plan`, replacing any Xray already running. The guard goes up first,
    /// so there is no moment with a tunnel and no guard.
    Connect {
        /// Where the tunnel goes.
        plan: TunnelPlan,
        /// What else the guard lets out.
        guard: GuardPolicy,
    },
    /// Stop Xray and keep the guard, replacing its policy: the tunnel is down
    /// and nothing leaves outside it while the app opens the next session.
    Hold {
        /// What the guard lets out while it holds.
        guard: GuardPolicy,
    },
    /// Stop Xray and remove the guard: traffic flows as it would without
    /// Wanderer.
    Disconnect,
    /// Resolve a node's name to IPv4 addresses, past the guard.
    ///
    /// A node may publish a name instead of an address, and the app learns
    /// it when the guard already blocks DNS. The daemon's own queries are the
    /// one kind the guard lets out (see [`guard::RESOLVER_MARK`]).
    ///
    /// [`guard::RESOLVER_MARK`]: crate::guard::RESOLVER_MARK
    Resolve {
        /// The name.
        host: String,
    },
}

/// The daemon's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "reply")]
pub enum Reply {
    /// Done; the state after it.
    Ok(Status),
    /// The answer to [`Request::Resolve`].
    Resolved {
        /// Its IPv4 addresses; empty when the name has none.
        addresses: Vec<std::net::Ipv4Addr>,
    },
    /// Not done; the state is what it was before the request, apart from a
    /// failed Xray start, after which the daemon holds.
    Error {
        /// What went wrong, for the log.
        message: String,
        /// The state after the failure.
        status: Status,
    },
}

/// Where the daemon is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The daemon's [`PROTOCOL_VERSION`].
    pub version: u32,
    /// The state.
    pub state: State,
    /// Bytes through the tunnel since the last `Connect` from `Off`.
    pub traffic: Traffic,
    /// Why Xray stopped, if it stopped on its own since the last `Connect`.
    pub xray_exit: Option<String>,
}

/// The daemon's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No guard, no tunnel.
    Off,
    /// The guard holds and Xray runs.
    Up,
    /// The guard holds and Xray does not run: nothing leaves.
    Holding,
}

/// Bytes counted on the TUN interface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traffic {
    /// Sent by the machine into the tunnel.
    pub up: u64,
    /// Received by the machine from the tunnel.
    pub down: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Endpoint;

    #[test]
    fn requests_read_back_as_they_were_sent() {
        let requests = [
            Request::Status,
            Request::Connect {
                plan: crate::plan::tests::chain(2),
                guard: GuardPolicy {
                    allowed: vec![Endpoint("198.51.100.4:443".parse().unwrap())],
                    allow_lan: true,
                },
            },
            Request::Hold {
                guard: GuardPolicy::default(),
            },
            Request::Disconnect,
        ];
        for r in requests {
            let line = serde_json::to_string(&r).unwrap();
            assert!(!line.contains('\n'));
            assert_eq!(serde_json::from_str::<Request>(&line).unwrap(), r);
        }
    }

    #[test]
    fn the_wire_format_is_stable() {
        let line = serde_json::to_string(&Request::Hold {
            guard: GuardPolicy::default(),
        })
        .unwrap();
        assert_eq!(
            line,
            r#"{"request":"hold","guard":{"allowed":[],"allow_lan":false}}"#
        );
        let reply = Reply::Ok(Status {
            version: PROTOCOL_VERSION,
            state: State::Holding,
            traffic: Traffic { up: 1, down: 2 },
            xray_exit: None,
        });
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"reply":"ok","version":3,"state":"holding","traffic":{"up":1,"down":2},"xray_exit":null}"#
        );
    }
}
