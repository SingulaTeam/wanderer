//! Where the tunnel goes and how, and what the guard lets through.
//!
//! Every field here reaches a root process from an unprivileged one, so each
//! type has a `validate` that the daemon runs before acting on it. The rules
//! are deliberately narrow: what Singula nodes actually send, no more.

use std::net::{Ipv4Addr, SocketAddrV4};

use serde::{Deserialize, Serialize};

/// How to reach one session's node, or a chain of them.
///
/// Built by the app from the sessions' tunnel settings once it has chosen a
/// cover, a server name and a transport. Nodes are IPv4 addresses: the app
/// resolves names itself, before the guard is up, because the guard admits
/// addresses, not names.
///
/// The first node is the one the machine connects to, and the only one the
/// guard admits. Each [`onward`](Self::onward) hop is reached through the
/// ones before it, so a node sees the hop before it and no further back, and
/// traffic leaves from the last.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelPlan {
    /// The first node's REALITY inbound.
    pub node: SocketAddrV4,
    /// The VLESS user id of the first node's session.
    pub uuid: String,
    /// The REALITY parameters of the chosen cover.
    pub reality: Reality,
    /// Raw TCP with a flow, or xHTTP at a path.
    pub transport: Transport,
    /// The hops after the first, in order; empty for a single node.
    #[serde(default)]
    pub onward: Vec<Hop>,
    /// Addresses whose traffic leaves the chain before its last hop.
    #[serde(default)]
    pub early_exits: Vec<EarlyExit>,
}

/// A hop after the first: a session opened as a chain's leg.
///
/// A node serves such a session over raw TCP without a flow — Vision needs a
/// direct connection to take over, and the hop before is in the way — under
/// the same REALITY parameters as any other.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hop {
    /// The node's REALITY inbound.
    pub node: SocketAddrV4,
    /// The VLESS user id of its session.
    pub uuid: String,
    /// The REALITY parameters of its cover.
    pub reality: Reality,
}

/// Traffic to `address` leaves the chain at hop `hop` — 0 is the first —
/// rather than at the last.
///
/// For each session's Control Plane. It already knows the node of the
/// session it serves; reached from there, reports on the session tell it
/// nothing more, where reached from the last hop they would show it where the
/// chain goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EarlyExit {
    /// The hop it leaves at; never the last.
    pub hop: u8,
    /// The address, on any port.
    pub address: Ipv4Addr,
}

/// The REALITY parameters for one connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reality {
    /// The node's REALITY public key, base64url without padding.
    pub public_key: String,
    /// The short id paired with [`server_name`](Self::server_name), in hex.
    pub short_id: String,
    /// The server name to present.
    pub server_name: String,
    /// The TLS fingerprint to imitate, such as `chrome`.
    pub fingerprint: String,
}

/// The two ways a node serves every cover, on the same port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Transport {
    /// VLESS over raw TCP with the XTLS Vision flow.
    Vision,
    /// VLESS over xHTTP at the cover's path, which the node matches exactly.
    Xhttp {
        /// The path, starting with `/`.
        path: String,
    },
}

/// What the Network Guard lets out besides the tunnel.
///
/// The node of the running plan is admitted on its own; these are the
/// endpoints the app must reach while the tunnel is down and the guard holds
/// — a Control Plane and an RPC endpoint, to open the next session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardPolicy {
    /// Endpoints reachable outside the tunnel.
    pub allowed: Vec<Endpoint>,
    /// Whether the local network stays reachable. DNS to resolvers on it is
    /// refused either way: it would tell the network every name looked up.
    pub allow_lan: bool,
}

/// One address and port, over TCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint(pub SocketAddrV4);

/// Why a plan or a policy was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {field}")]
pub struct InvalidPlan {
    /// The field that failed.
    pub field: &'static str,
}

/// The fingerprints Xray's uTLS can imitate that a client should use.
pub const FINGERPRINTS: &[&str] = &[
    "chrome",
    "firefox",
    "safari",
    "ios",
    "android",
    "edge",
    "360",
    "qq",
    "random",
    "randomized",
];

/// More would be a guard in name only.
pub const MAX_ALLOWED_ENDPOINTS: usize = 32;

/// The most nodes a chain may have. A fourth adds little a third does not,
/// and costs every connection another round trip.
pub const MAX_HOPS: usize = 3;

/// A Control Plane publishes a handful of addresses; this leaves room for
/// one per hop with some to spare.
pub const MAX_EARLY_EXITS: usize = 16;

impl TunnelPlan {
    /// Checks every field against what Singula nodes send.
    ///
    /// # Errors
    ///
    /// Names the first field that does not pass.
    pub fn validate(&self) -> Result<(), InvalidPlan> {
        check("node", routable(self.node))?;
        check("uuid", is_uuid(&self.uuid))?;
        self.reality.validate()?;
        if let Transport::Xhttp { path } = &self.transport {
            check("transport path", is_path(path))?;
        }
        check("onward hops", self.onward.len() < MAX_HOPS)?;
        for hop in &self.onward {
            check("hop node", routable(hop.node))?;
            check("hop uuid", is_uuid(&hop.uuid))?;
            hop.reality.validate()?;
        }
        check(
            "early exits",
            self.early_exits.len() <= MAX_EARLY_EXITS
                && self.early_exits.iter().all(|e| {
                    usize::from(e.hop) < self.onward.len()
                        && routable(SocketAddrV4::new(e.address, 1))
                }),
        )
    }
}

impl Reality {
    fn validate(&self) -> Result<(), InvalidPlan> {
        // An X25519 key: 32 bytes, 43 characters of base64url.
        check(
            "reality public key",
            self.public_key.len() == 43
                && self
                    .public_key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        )?;
        check(
            "reality short id",
            self.short_id.len() <= 16
                && self.short_id.len().is_multiple_of(2)
                && self.short_id.bytes().all(|b| b.is_ascii_hexdigit()),
        )?;
        check("server name", is_host_name(&self.server_name))?;
        check(
            "fingerprint",
            FINGERPRINTS.contains(&self.fingerprint.as_str()),
        )
    }
}

impl GuardPolicy {
    /// Checks the policy is one the guard can enforce.
    ///
    /// # Errors
    ///
    /// Refuses unroutable endpoints and more than [`MAX_ALLOWED_ENDPOINTS`].
    pub fn validate(&self) -> Result<(), InvalidPlan> {
        check(
            "allowed endpoints",
            self.allowed.len() <= MAX_ALLOWED_ENDPOINTS
                && self.allowed.iter().all(|e| routable(e.0)),
        )
    }
}

fn check(field: &'static str, ok: bool) -> Result<(), InvalidPlan> {
    if ok {
        Ok(())
    } else {
        Err(InvalidPlan { field })
    }
}

/// A public unicast address with a port: anything else is either a mistake or
/// an attempt to open a hole in the guard towards the machine's own network.
fn routable(addr: SocketAddrV4) -> bool {
    let ip = addr.ip();
    addr.port() != 0
        && !(ip.is_unspecified()
            || ip.is_loopback()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_broadcast()
            || ip.is_multicast())
}

fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A DNS name: letters, digits, hyphens and dots.
#[must_use]
pub fn is_host_name(s: &str) -> bool {
    (1..=253).contains(&s.len())
        && s.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// An HTTP path the node can match exactly: no query, no fragment, nothing
/// that is not printable ASCII.
fn is_path(s: &str) -> bool {
    s.starts_with('/')
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_graphic() && b != b'?' && b != b'#' && b != b'%')
}

impl std::fmt::Debug for TunnelPlan {
    // The uuid is the session's credential: anyone holding it can use it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TunnelPlan")
            .field("node", &self.node)
            .field("server_name", &self.reality.server_name)
            .field("transport", &self.transport)
            .field(
                "onward",
                &self.onward.iter().map(|h| h.node).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn plan(transport: Transport) -> TunnelPlan {
        TunnelPlan {
            node: "203.0.113.7:443".parse().unwrap(),
            uuid: "7316e3b4-53eb-41da-90d9-23bbde8e1cc6".into(),
            reality: Reality {
                public_key: "Vfv6lG2gjLQSbkLJdcH72Eo7zW2LrNRhAyrvU_YPJGQ".into(),
                short_id: "a1b2c3d4".into(),
                server_name: "www.adobe.com".into(),
                fingerprint: "chrome".into(),
            },
            transport,
            onward: Vec::new(),
            early_exits: Vec::new(),
        }
    }

    /// `plan` with `n` hops after it.
    pub(crate) fn chain(n: u8) -> TunnelPlan {
        let mut p = plan(Transport::Vision);
        for i in 0..n {
            p.onward.push(Hop {
                node: SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 10 + i), 443),
                uuid: format!("0b3f5a2e-1c4d-4e6f-8a9b-{:012x}", u64::from(i)),
                reality: p.reality.clone(),
            });
        }
        p
    }

    fn field(p: &TunnelPlan) -> Option<&'static str> {
        p.validate().err().map(|e| e.field)
    }

    #[test]
    fn accepts_what_nodes_send() {
        assert_eq!(field(&plan(Transport::Vision)), None);
        let xhttp = Transport::Xhttp {
            path: "/api/v1/session/heartbeat".into(),
        };
        assert_eq!(field(&plan(xhttp)), None);
    }

    #[test]
    fn refuses_a_node_on_the_local_network() {
        for node in [
            "192.168.1.1:443",
            "127.0.0.1:443",
            "10.0.0.1:443",
            "203.0.113.7:0",
        ] {
            let mut p = plan(Transport::Vision);
            p.node = node.parse().unwrap();
            assert_eq!(field(&p), Some("node"), "{node}");
        }
    }

    #[test]
    fn refuses_malformed_fields() {
        let mut p = plan(Transport::Vision);
        p.uuid = "not-a-uuid".into();
        assert_eq!(field(&p), Some("uuid"));

        let mut p = plan(Transport::Vision);
        p.reality.public_key.push('=');
        assert_eq!(field(&p), Some("reality public key"));

        let mut p = plan(Transport::Vision);
        p.reality.short_id = "abc".into();
        assert_eq!(field(&p), Some("reality short id"));

        let mut p = plan(Transport::Vision);
        p.reality.server_name = "a b.example".into();
        assert_eq!(field(&p), Some("server name"));

        let mut p = plan(Transport::Vision);
        p.reality.fingerprint = "hellochrome_auto".into();
        assert_eq!(field(&p), Some("fingerprint"));

        for path in ["api", "/a?b=1", "/a b", "/%2e%2e"] {
            let p = plan(Transport::Xhttp { path: path.into() });
            assert_eq!(field(&p), Some("transport path"), "{path}");
        }
    }

    #[test]
    fn guard_refuses_holes_to_the_local_network() {
        let policy = |addr: &str| GuardPolicy {
            allowed: vec![Endpoint(addr.parse().unwrap())],
            allow_lan: false,
        };
        assert!(policy("203.0.113.9:443").validate().is_ok());
        assert!(policy("192.168.0.1:22").validate().is_err());

        let many = GuardPolicy {
            allowed: vec![Endpoint("203.0.113.9:443".parse().unwrap()); MAX_ALLOWED_ENDPOINTS + 1],
            allow_lan: false,
        };
        assert!(many.validate().is_err());
    }

    #[test]
    fn debug_hides_the_uuid() {
        let shown = format!("{:?}", chain(1));
        assert!(
            !shown.contains("7316e3b4") && !shown.contains("0b3f5a2e"),
            "{shown}"
        );
    }

    #[test]
    fn chains_up_to_three_nodes() {
        assert_eq!(field(&chain(2)), None);
        assert_eq!(field(&chain(3)), Some("onward hops"));

        let mut p = chain(1);
        p.onward[0].node = "10.0.0.5:443".parse().unwrap();
        assert_eq!(field(&p), Some("hop node"));
        let mut p = chain(1);
        p.onward[0].uuid = "x".into();
        assert_eq!(field(&p), Some("hop uuid"));
    }

    #[test]
    fn traffic_leaves_early_only_before_the_last_hop() {
        let exit = |hop| EarlyExit {
            hop,
            address: Ipv4Addr::new(203, 0, 113, 50),
        };
        let mut p = chain(2);
        p.early_exits = vec![exit(0), exit(1)];
        assert_eq!(field(&p), None);
        p.early_exits.push(exit(2));
        assert_eq!(
            field(&p),
            Some("early exits"),
            "the last hop is where all traffic leaves"
        );

        let mut p = plan(Transport::Vision);
        p.early_exits = vec![exit(0)];
        assert_eq!(
            field(&p),
            Some("early exits"),
            "a single node has no early exit"
        );

        let mut p = chain(1);
        p.early_exits = vec![EarlyExit {
            hop: 0,
            address: Ipv4Addr::new(192, 168, 1, 1),
        }];
        assert_eq!(field(&p), Some("early exits"));
    }
}
