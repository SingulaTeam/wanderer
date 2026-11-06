//! The Network Guard on Windows, as Windows Filtering Platform filters.
//!
//! What [`guard`](crate::guard) is in nftables, described the same way: a
//! plain list the daemon turns into filters, so the rules can be read and
//! tested anywhere. The daemon adds them in one WFP transaction, in a sublayer
//! of its own, at the layers where a connection — or a UDP flow's first
//! packet — is authorised to leave.
//!
//! Within the sublayer the filter with the highest weight that matches
//! decides. The guard lets out the TUN, the node, the endpoints the app needs
//! to reconnect, DHCP, the daemon's own DNS queries for node names and the
//! local network if allowed; DNS to anything else and the rest of IPv4 are
//! blocked, IPv6 entirely, because it is not tunnelled yet.
//!
//! The filters outlive the daemon: if it dies while connected the machine
//! stays offline rather than leaking, until the daemon starts again and
//! removes them, as it does on Linux.

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::plan::GuardPolicy;

/// What a filter does with what it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Let it out.
    Permit,
    /// Drop it.
    Block,
}

/// Which address family a filter's layer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// IPv4.
    V4,
    /// IPv6.
    V6,
}

/// One condition; a filter matches when all of its conditions do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Traffic on the loopback interface.
    Loopback,
    /// Traffic out of the TUN interface.
    Tun,
    /// Traffic from the daemon's own process.
    Daemon,
    /// To an address in this network.
    RemoteNetwork {
        /// Its address.
        address: Ipv4Addr,
        /// Its prefix length.
        prefix: u8,
    },
    /// To this port.
    RemotePort(u16),
    /// From this port.
    LocalPort(u16),
    /// Over this IP protocol.
    Protocol(Protocol),
}

/// The IP protocols the guard names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// TCP, 6.
    Tcp,
    /// UDP, 17.
    Udp,
}

impl Protocol {
    /// The protocol's number.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Tcp => 6,
            Self::Udp => 17,
        }
    }
}

/// One WFP filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// A short description, for anyone listing the system's filters.
    pub name: &'static str,
    /// The layer's family.
    pub family: Family,
    /// Higher weights are matched first.
    pub weight: u8,
    /// Permit or block.
    pub action: Action,
    /// All of them must match.
    pub conditions: Vec<Condition>,
}

/// Private, link-local and multicast IPv4: the local network.
const LAN: [(Ipv4Addr, u8); 5] = [
    (Ipv4Addr::new(10, 0, 0, 0), 8),
    (Ipv4Addr::new(172, 16, 0, 0), 12),
    (Ipv4Addr::new(192, 168, 0, 0), 16),
    (Ipv4Addr::new(169, 254, 0, 0), 16),
    (Ipv4Addr::new(224, 0, 0, 0), 4),
];

/// The filters that install the guard for `policy`, admitting `node` too.
///
/// `tun` says whether the TUN exists to be admitted: on Windows the filter
/// names the interface itself, which Xray creates only when it starts, so the
/// daemon installs the guard without it first and again once Xray is up.
#[must_use]
pub fn filters(policy: &GuardPolicy, node: Option<SocketAddrV4>, tun: bool) -> Vec<Filter> {
    let mut out = Vec::new();
    let mut add = |name, family, weight, action, conditions| {
        out.push(Filter {
            name,
            family,
            weight,
            action,
            conditions,
        });
    };

    for family in [Family::V4, Family::V6] {
        add(
            "loopback",
            family,
            15,
            Action::Permit,
            vec![Condition::Loopback],
        );
    }
    if tun {
        add(
            "the tunnel",
            Family::V4,
            14,
            Action::Permit,
            vec![Condition::Tun],
        );
    }
    add(
        "the daemon resolving node names",
        Family::V4,
        13,
        Action::Permit,
        vec![
            Condition::Daemon,
            Condition::Protocol(Protocol::Udp),
            Condition::RemotePort(53),
        ],
    );
    add(
        "DHCP",
        Family::V4,
        12,
        Action::Permit,
        vec![
            Condition::Protocol(Protocol::Udp),
            Condition::LocalPort(68),
            Condition::RemotePort(67),
        ],
    );

    let mut allowed: Vec<SocketAddrV4> = policy.allowed.iter().map(|e| e.0).chain(node).collect();
    allowed.sort_unstable();
    allowed.dedup();
    for endpoint in allowed {
        add(
            "the node, or an endpoint needed to reconnect",
            Family::V4,
            11,
            Action::Permit,
            vec![
                Condition::Protocol(Protocol::Tcp),
                Condition::RemoteNetwork {
                    address: *endpoint.ip(),
                    prefix: 32,
                },
                Condition::RemotePort(endpoint.port()),
            ],
        );
    }

    // DNS anywhere but through the TUN, the local network included: a query
    // to the router's resolver names every site visited.
    for protocol in [Protocol::Tcp, Protocol::Udp] {
        add(
            "DNS outside the tunnel",
            Family::V4,
            10,
            Action::Block,
            vec![Condition::Protocol(protocol), Condition::RemotePort(53)],
        );
    }
    if policy.allow_lan {
        for (address, prefix) in LAN {
            add(
                "the local network",
                Family::V4,
                9,
                Action::Permit,
                vec![Condition::RemoteNetwork { address, prefix }],
            );
        }
    }

    add("everything else", Family::V4, 0, Action::Block, Vec::new());
    add(
        "IPv6, not tunnelled",
        Family::V6,
        0,
        Action::Block,
        Vec::new(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Endpoint;

    fn policy(allow_lan: bool) -> GuardPolicy {
        GuardPolicy {
            allowed: vec![Endpoint("198.51.100.4:443".parse().unwrap())],
            allow_lan,
        }
    }

    fn named<'a>(fs: &'a [Filter], name: &str) -> Vec<&'a Filter> {
        fs.iter().filter(|f| f.name == name).collect()
    }

    #[test]
    fn blocks_everything_it_does_not_name() {
        let fs = filters(&policy(false), None, false);
        let rest = named(&fs, "everything else");
        assert_eq!(rest.len(), 1);
        assert_eq!((rest[0].action, rest[0].weight), (Action::Block, 0));
        assert_eq!(rest[0].conditions, Vec::new());
        let v6 = named(&fs, "IPv6, not tunnelled");
        assert_eq!((v6[0].family, v6[0].action), (Family::V6, Action::Block));
    }

    #[test]
    fn admits_the_node_and_the_allowed_endpoints_once() {
        let node: SocketAddrV4 = "203.0.113.7:443".parse().unwrap();
        let mut p = policy(false);
        p.allowed.push(Endpoint(node));
        let fs = filters(&p, Some(node), false);
        let endpoints = named(&fs, "the node, or an endpoint needed to reconnect");
        assert_eq!(endpoints.len(), 2, "{endpoints:?}");
        assert!(endpoints[1].conditions.contains(&Condition::RemoteNetwork {
            address: *node.ip(),
            prefix: 32
        }));
    }

    #[test]
    fn the_tunnel_is_admitted_only_once_it_exists() {
        assert_eq!(
            named(&filters(&policy(false), None, false), "the tunnel"),
            Vec::<&Filter>::new()
        );
        assert_eq!(
            named(&filters(&policy(false), None, true), "the tunnel").len(),
            1
        );
    }

    #[test]
    fn dns_is_blocked_above_the_lan_and_below_the_tunnel() {
        let fs = filters(&policy(true), None, true);
        let weight = |name| named(&fs, name)[0].weight;
        assert!(weight("DNS outside the tunnel") > weight("the local network"));
        assert!(weight("the tunnel") > weight("DNS outside the tunnel"));
        assert!(weight("the daemon resolving node names") > weight("DNS outside the tunnel"));
        assert_eq!(
            named(&filters(&policy(false), None, true), "the local network"),
            Vec::<&Filter>::new()
        );
    }
}
