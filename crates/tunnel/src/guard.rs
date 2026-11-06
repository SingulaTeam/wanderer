//! The Network Guard's nftables ruleset.
//!
//! The guard is what stops traffic from leaving outside the tunnel, and it
//! does not depend on Xray: Xray removes its routes when it stops or crashes,
//! and without the guard the system falls straight back to the physical link
//! — DNS first. The guard stays from "connect" to "disconnect", through every
//! reconnect in between.
//!
//! It drops everything the machine sends except to the TUN, the node, the
//! endpoints the app needs to reconnect, DHCP, and the local network if
//! allowed; IPv6 is dropped outright, because it is not tunnelled yet. DNS to
//! anything but the TUN is dropped even on the local network.

use std::net::SocketAddrV4;

use crate::TUN_NAME;
use crate::plan::GuardPolicy;

/// The nftables table the guard owns. Nothing else touches it.
pub const TABLE: &str = "wanderer";

/// The socket mark on the daemon's own DNS queries, the only ones the guard
/// lets out while it holds: the names of nodes, which the app must reach to
/// reconnect. `SO_MARK` needs `CAP_NET_ADMIN`, so no other program can set it.
pub const RESOLVER_MARK: u32 = 0x5744_4e53; // "WDNS"

/// Private, link-local and multicast IPv4: the local network.
const LAN: &[&str] = &[
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "224.0.0.0/4",
];

/// The script that installs the guard for `policy`, admitting `node` as well.
///
/// Fed to `nft -f -`, it replaces any previous guard in one transaction: the
/// table is declared, deleted and defined again, so there is no moment
/// without rules while a reconnect moves the guard to a new node.
#[must_use]
pub fn install(policy: &GuardPolicy, node: Option<SocketAddrV4>) -> String {
    let mut allowed: Vec<SocketAddrV4> = policy.allowed.iter().map(|e| e.0).chain(node).collect();
    allowed.sort_unstable();
    allowed.dedup();
    // nft refuses an empty `elements = { }`.
    let allowed = if allowed.is_empty() {
        String::new()
    } else {
        let elements: Vec<String> = allowed
            .iter()
            .map(|a| format!("{} . {}", a.ip(), a.port()))
            .collect();
        format!("elements = {{ {} }}", elements.join(", "))
    };
    let lan = LAN.join(", ");
    let lan_output = if policy.allow_lan {
        "ip daddr @lan4 accept"
    } else {
        ""
    };

    // Declaring an existing table is a no-op, so the delete always has
    // something to delete. The DHCP rule keeps the machine's lease alive;
    // the marked DNS is the daemon resolving a node's name.
    // Containers and virtual machines route through the host and pass the
    // forward hook, not the output one; between local virtual networks, which
    // live in private ranges, they are left alone.
    format!(
        "\
table inet {TABLE} {{}}
delete table inet {TABLE}
table inet {TABLE} {{
\tset lan4 {{
\t\ttype ipv4_addr; flags interval
\t\telements = {{ {lan} }}
\t}}
\tset allowed4 {{
\t\ttype ipv4_addr . inet_service
\t\t{allowed}
\t}}
\tchain output {{
\t\ttype filter hook output priority 0; policy drop;
\t\toif \"lo\" accept
\t\toifname \"{TUN_NAME}\" accept
\t\tmeta nfproto ipv6 drop
\t\tudp sport 68 udp dport 67 accept
\t\tmeta mark {RESOLVER_MARK:#x} udp dport 53 accept
\t\tip daddr . tcp dport @allowed4 accept
\t\tmeta l4proto {{ tcp, udp }} th dport 53 drop
\t\t{lan_output}
\t}}
\tchain forward {{
\t\ttype filter hook forward priority 0; policy accept;
\t\toifname \"{TUN_NAME}\" accept
\t\tmeta nfproto ipv6 drop
\t\tmeta l4proto {{ tcp, udp }} th dport 53 drop
\t\tip daddr @lan4 accept
\t\tdrop
\t}}
}}
"
    )
}

/// The script that removes the guard; harmless when there is none.
#[must_use]
pub fn remove() -> String {
    format!("table inet {TABLE} {{}}\ndelete table inet {TABLE}\n")
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

    #[test]
    fn replaces_the_previous_guard_in_one_transaction() {
        let s = install(&policy(true), None);
        let lines: Vec<&str> = s.lines().take(3).collect();
        assert_eq!(
            lines,
            [
                "table inet wanderer {}",
                "delete table inet wanderer",
                "table inet wanderer {"
            ]
        );
    }

    #[test]
    fn admits_the_node_and_the_allowed_endpoints_once() {
        let node = "203.0.113.7:443".parse().unwrap();
        let mut p = policy(false);
        p.allowed.push(Endpoint(node));
        let s = install(&p, Some(node));
        assert!(
            s.contains("elements = { 198.51.100.4 . 443, 203.0.113.7 . 443 }"),
            "{s}"
        );
    }

    #[test]
    fn an_empty_set_has_no_elements_line() {
        // nft refuses `elements = { }`.
        let s = install(&GuardPolicy::default(), None);
        assert!(!s.contains("elements = {  }"), "{s}");
        assert!(s.contains("set allowed4"));
    }

    #[test]
    fn the_lan_toggle_opens_only_the_lan() {
        let accept_lan = "\t\tip daddr @lan4 accept\n\t}\n\tchain forward";
        assert!(install(&policy(true), None).contains(accept_lan));
        assert!(!install(&policy(false), None).contains(accept_lan));
    }

    #[test]
    fn dns_and_ipv6_are_dropped_before_the_lan_is_admitted() {
        let s = install(&policy(true), None);
        let output = &s[s.find("chain output").unwrap()..s.find("chain forward").unwrap()];
        let at = |rule: &str| output.find(rule).unwrap();
        assert!(at("th dport 53 drop") < at("@lan4 accept"));
        assert!(at("ipv6 drop") < at("@lan4 accept"));
        assert!(
            at(&format!("meta mark {RESOLVER_MARK:#x} udp dport 53 accept"))
                < at("th dport 53 drop"),
            "the daemon's queries pass before DNS is dropped"
        );
        assert!(at(&format!("oifname \"{TUN_NAME}\" accept")) < at("ipv6 drop"));
        assert!(output.contains("policy drop"));
    }
}
