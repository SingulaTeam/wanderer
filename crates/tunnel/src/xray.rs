//! The Xray configuration for a plan.
//!
//! The shape is the one the leak tests passed with: a TUN that takes the two
//! halves of the IPv4 space (a single `0.0.0.0/0` loses to the existing
//! default route on metric), the proxy as the first and therefore default
//! outbound, and no direct outbound at all, so nothing Xray handles can leave
//! outside the tunnel. DNS sent into the TUN is answered by Xray itself, over
//! DNS-over-HTTPS through the proxy.
//!
//! A chain is one outbound per node, each dialled through the one before it;
//! the last is the default. Addresses that leave the chain early are routed
//! to their hop's outbound by destination.
//!
//! Only the TUN differs between systems. On Linux and Windows Xray creates it
//! and sets its routes; on Android the VPN service does both, with the same
//! addresses, and Xray reads the descriptor it was given.

use serde_json::{Value, json};

use crate::TUN_NAME;
use crate::plan::{Reality, Transport, TunnelPlan};

/// The system Xray runs on, and how it is set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// Linux, with the guard in nftables.
    Linux {
        /// Whether systemd-resolved runs. Xray then points it at the TUN;
        /// without it, Xray refuses to start the TUN when asked to.
        systemd_resolved: bool,
    },
    /// Windows, with Wintun.
    Windows,
    /// Android: the VPN service owns the interface and hands Xray its
    /// descriptor (see [`ANDROID_TUN_FD`]).
    Android,
}

/// The environment variable Xray reads the TUN's descriptor from on
/// Android.
pub const ANDROID_TUN_FD: &str = "xray.tun.fd";

/// The TUN's address; the next one is where Xray answers DNS.
///
/// From 198.18.0.0/15, reserved for benchmarking (RFC 2544): home routers,
/// Docker and VPNs claim the private ranges, and an address one of them also
/// holds — Docker's networks start at 172.17–172.31 — would collide.
const GATEWAY: &str = "198.18.0.1/30";
/// The TUN's own address, for a system that sets the interface up itself.
pub const ADDRESS: [u8; 4] = [198, 18, 0, 1];
/// Where Xray answers DNS sent into the TUN.
pub const DNS: [u8; 4] = [198, 18, 0, 2];
/// The TUN's prefix length.
pub const PREFIX: u8 = 30;
/// The TUN's MTU.
pub const MTU: u16 = 1500;
/// The default route split in two, each more specific than any default.
pub const ROUTES: [&str; 2] = ["0.0.0.0/1", "128.0.0.0/1"];
/// Asked through the tunnel, so the resolver sees the node, not the user.
const RESOLVER: &str = "https://1.1.1.1/dns-query";
const VISION_FLOW: &str = "xtls-rprx-vision";
/// The default outbound: the chain's last hop.
const PROXY: &str = "proxy";
/// Where queries other than A and AAAA go, through the tunnel.
const FORWARD_RESOLVER: &str = "1.1.1.1";

/// The configuration that carries the machine's traffic along `plan`.
///
/// The plan must have passed [`TunnelPlan::validate`].
#[must_use]
pub fn config(plan: &TunnelPlan, host: Host) -> Value {
    let mut config = json!({
        // No access log: it would record every destination the user visits.
        "log": { "loglevel": "warning", "access": "none" },
        "dns": {
            "servers": [RESOLVER],
            // IPv6 is not tunnelled yet; the guard drops it.
            "queryStrategy": "UseIPv4",
        },
        "inbounds": [{
            "tag": "tun",
            "protocol": "tun",
            "settings": tun(host),
        }],
        "outbounds": [],
        "routing": { "rules": [{
            "type": "field",
            "inboundTag": ["tun"],
            "port": 53,
            "outboundTag": "dns",
        }] },
    });
    let mut outbounds = hops(plan);
    outbounds.push(dns());
    config["outbounds"] = Value::Array(outbounds);
    if let Some(rules) = config["routing"]["rules"].as_array_mut() {
        rules.extend(early_exits(plan));
    }
    if host == Host::Android {
        // Xray runs inside the app there, and the app reads its counters
        // directly; nothing listens for them.
        config["stats"] = json!({});
        config["policy"] = json!({
            "system": { "statsInboundUplink": true, "statsInboundDownlink": true },
        });
    }
    config
}

fn tun(host: Host) -> Value {
    let dns = std::net::Ipv4Addr::from(DNS).to_string();
    match host {
        Host::Linux { systemd_resolved } => json!({
            "name": TUN_NAME,
            "mtu": MTU,
            "gateway": [GATEWAY],
            "autoSystemRoutingTable": ROUTES,
            "autoSystemDnsToGateway": systemd_resolved,
        }),
        Host::Windows => json!({
            "name": TUN_NAME,
            "mtu": MTU,
            "gateway": [GATEWAY],
            // Set on the adapter; inside the gateway's /30, as the leak
            // filters need.
            "dns": [dns],
            "autoSystemRoutingTable": ROUTES,
            // While Xray runs: DNS only through the TUN, and IPv6, which has
            // no route into it, not at all. The guard covers the rest, and
            // the time Xray does not run.
            "autoSystemWfpBlockLeak": ["dns", "misconfigtun"],
        }),
        // Addresses, routes and DNS are the VPN service's. The name is
        // unused there, but without one Xray lists the system's interfaces
        // to pick a free one, which Android does not let an app do.
        Host::Android => json!({ "name": TUN_NAME, "mtu": MTU }),
    }
}

/// Answers the DNS sent into the TUN: A and AAAA by Xray's own resolver
/// (over `DoH`, through the proxy); every other type — TXT, MX, SRV, which mail
/// clients and messengers need — passed to a public resolver, through the
/// proxy too. Without the second rule Xray answers those with nothing.
fn dns() -> Value {
    json!({
        "tag": "dns",
        "protocol": "dns",
        "settings": {
            "rewriteAddress": FORWARD_RESOLVER,
            "rewritePort": 53,
            "rules": [
                { "action": "hijack", "qType": "1,28" },
                { "action": "direct" },
            ],
        },
        // The forwarded queries leave through the tunnel, never beside it.
        "streamSettings": { "sockopt": { "dialerProxy": "proxy" } },
    })
}

/// The tag of hop `i` of `count`.
fn tag(i: usize, count: usize) -> String {
    if i + 1 == count {
        PROXY.to_owned()
    } else {
        format!("hop{i}")
    }
}

/// One outbound per node, the last hop first: Xray's default outbound is its
/// first.
fn hops(plan: &TunnelPlan) -> Vec<Value> {
    let count = plan.onward.len() + 1;
    let (flow, network) = match &plan.transport {
        Transport::Vision => (VISION_FLOW, "raw"),
        Transport::Xhttp { .. } => ("", "xhttp"),
    };
    let mut first = vless(&tag(0, count), plan.node, &plan.uuid, &plan.reality, flow);
    first["streamSettings"]["network"] = json!(network);
    if let Transport::Xhttp { path } = &plan.transport {
        first["streamSettings"]["xhttpSettings"] = json!({ "path": path, "mode": "auto" });
    }

    let mut outbounds = vec![first];
    for (i, hop) in plan.onward.iter().enumerate() {
        let mut next = vless(&tag(i + 1, count), hop.node, &hop.uuid, &hop.reality, "");
        next["streamSettings"]["sockopt"] = json!({ "dialerProxy": tag(i, count) });
        outbounds.push(next);
    }
    outbounds.reverse();
    outbounds
}

/// VLESS over REALITY on raw TCP to `node`.
fn vless(
    tag: &str,
    node: std::net::SocketAddrV4,
    uuid: &str,
    reality: &Reality,
    flow: &str,
) -> Value {
    json!({
        "tag": tag,
        "protocol": "vless",
        "settings": {
            "vnext": [{
                "address": node.ip().to_string(),
                "port": node.port(),
                "users": [{ "id": uuid, "encryption": "none", "flow": flow }],
            }],
        },
        "streamSettings": {
            "network": "raw",
            "security": "reality",
            "realitySettings": {
                "serverName": reality.server_name,
                "fingerprint": reality.fingerprint,
                "publicKey": reality.public_key,
                "shortId": reality.short_id,
            },
        },
    })
}

/// A rule per hop that some addresses leave at.
fn early_exits(plan: &TunnelPlan) -> Vec<Value> {
    let count = plan.onward.len() + 1;
    (0..count)
        .filter_map(|hop| {
            let ips: Vec<String> = plan
                .early_exits
                .iter()
                .filter(|e| usize::from(e.hop) == hop)
                .map(|e| e.address.to_string())
                .collect();
            (!ips.is_empty()).then(|| {
                json!({
                    "type": "field",
                    "inboundTag": ["tun"],
                    "ip": ips,
                    "outboundTag": tag(hop, count),
                })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::tests::plan;

    const HOST: Host = Host::Linux {
        systemd_resolved: true,
    };

    #[test]
    fn vision_runs_raw_tcp_with_the_flow() {
        let c = config(&plan(Transport::Vision), HOST);
        let proxy = &c["outbounds"][0];
        assert_eq!(proxy["tag"], "proxy", "the proxy is the default outbound");
        assert_eq!(proxy["settings"]["vnext"][0]["address"], "203.0.113.7");
        assert_eq!(
            proxy["settings"]["vnext"][0]["users"][0]["flow"],
            VISION_FLOW
        );
        assert_eq!(proxy["streamSettings"]["network"], "raw");
        assert!(proxy["streamSettings"]["xhttpSettings"].is_null());
    }

    #[test]
    fn xhttp_sends_no_flow_and_the_exact_path() {
        let path = "/api/v1/session/heartbeat";
        let c = config(&plan(Transport::Xhttp { path: path.into() }), HOST);
        let proxy = &c["outbounds"][0];
        assert_eq!(proxy["settings"]["vnext"][0]["users"][0]["flow"], "");
        assert_eq!(proxy["streamSettings"]["network"], "xhttp");
        assert_eq!(proxy["streamSettings"]["xhttpSettings"]["path"], path);
        assert_eq!(proxy["streamSettings"]["security"], "reality");
    }

    #[test]
    fn nothing_can_leave_outside_the_proxy() {
        let c = config(&plan(Transport::Vision), HOST);
        let protocols: Vec<&str> = c["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["protocol"].as_str().unwrap())
            .collect();
        assert_eq!(protocols, ["vless", "dns"], "no freedom outbound");
        assert_eq!(
            c["outbounds"][1]["streamSettings"]["sockopt"]["dialerProxy"], "proxy",
            "forwarded DNS goes through the tunnel"
        );
        assert_eq!(
            c["inbounds"][0]["settings"]["autoSystemRoutingTable"],
            json!(ROUTES)
        );
        assert_eq!(c["log"]["access"], "none");
    }

    #[test]
    fn each_system_gets_its_own_tun() {
        let tun = |host| config(&plan(Transport::Vision), host)["inbounds"][0]["settings"].clone();
        assert_eq!(tun(HOST)["autoSystemDnsToGateway"], true);
        let windows = tun(Host::Windows);
        assert_eq!(windows["dns"], json!(["198.18.0.2"]));
        assert_eq!(
            windows["autoSystemWfpBlockLeak"],
            json!(["dns", "misconfigtun"])
        );
        let android = tun(Host::Android);
        assert_eq!(android["name"], TUN_NAME, "or Xray lists the interfaces");
        assert!(android["gateway"].is_null() && android["autoSystemRoutingTable"].is_null());
    }

    #[test]
    fn a_chain_dials_each_hop_through_the_one_before() {
        let c = config(&crate::plan::tests::chain(2), HOST);
        let out = c["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = out.iter().map(|o| o["tag"].as_str().unwrap()).collect();
        assert_eq!(
            tags,
            ["proxy", "hop1", "hop0", "dns"],
            "the last hop is the default"
        );

        let address = |o: &Value| o["settings"]["vnext"][0]["address"].clone();
        assert_eq!(
            address(&out[2]),
            "203.0.113.7",
            "the first hop is the plan's node"
        );
        assert_eq!(address(&out[1]), "198.51.100.10");
        assert_eq!(address(&out[0]), "198.51.100.11");

        let via = |o: &Value| o["streamSettings"]["sockopt"]["dialerProxy"].clone();
        assert_eq!(via(&out[0]), "hop1");
        assert_eq!(via(&out[1]), "hop0");
        assert!(via(&out[2]).is_null(), "the first hop is dialled directly");

        assert_eq!(
            out[2]["settings"]["vnext"][0]["users"][0]["flow"],
            VISION_FLOW
        );
        for leg in &out[..2] {
            assert_eq!(
                leg["settings"]["vnext"][0]["users"][0]["flow"], "",
                "a leg has no flow"
            );
            assert_eq!(leg["streamSettings"]["network"], "raw");
        }
    }

    #[test]
    fn early_exits_leave_at_their_hop() {
        let mut p = crate::plan::tests::chain(1);
        p.early_exits = vec![
            crate::plan::EarlyExit {
                hop: 0,
                address: "203.0.113.50".parse().unwrap(),
            },
            crate::plan::EarlyExit {
                hop: 0,
                address: "203.0.113.51".parse().unwrap(),
            },
        ];
        let c = config(&p, HOST);
        let rules = c["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["outboundTag"], "dns", "DNS first");
        assert_eq!(rules[1]["outboundTag"], "hop0");
        assert_eq!(rules[1]["ip"], json!(["203.0.113.50", "203.0.113.51"]));
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn only_android_counts_in_xray() {
        assert!(config(&plan(Transport::Vision), HOST)["stats"].is_null());
        let android = config(&plan(Transport::Vision), Host::Android);
        assert_eq!(android["policy"]["system"]["statsInboundUplink"], true);
    }
}
