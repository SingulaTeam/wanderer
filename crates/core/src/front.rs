//! Reaching Control Planes through their fronts.
//!
//! Where a Control Plane's endpoint is blocked, its front is not: VLESS over
//! REALITY to the same host, forwarding to its gRPC, with settings the
//! protocol fixes (`singula::chain::front`) and a key the Control Plane
//! published on chain. The app runs a relay — Xray, with one local entry per
//! front — and calls each Control Plane over its entry.
//!
//! The relay's entries listen on the loopback interface. What a program
//! there can reach through them is a Control Plane's gRPC, which it could
//! call directly anyway.

use std::net::SocketAddrV4;

use serde_json::{Value, json};
use singula::StarId;
use singula::chain::front;

/// One front, and the local port its entry listens on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The Control Plane.
    pub star: StarId,
    /// The entry's port on 127.0.0.1.
    pub port: u16,
    /// The front, at an address resolved beforehand: the guard blocks DNS.
    pub front: SocketAddrV4,
    /// The Control Plane's REALITY key, base64url, as read from the chain.
    pub public_key: String,
}

/// The relay's Xray configuration: each entry forwards to its front.
#[must_use]
pub fn relay_config(entries: &[Entry]) -> Value {
    let inbounds: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "tag": format!("entry-{}", e.star),
                "listen": "127.0.0.1",
                "port": e.port,
                "protocol": "dokodemo-door",
                // The front sends everything to its gRPC; this address is
                // only what the request names.
                "settings": { "address": "127.0.0.1", "port": 443, "network": "tcp" },
            })
        })
        .collect();
    let outbounds: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "tag": format!("front-{}", e.star),
                "protocol": "vless",
                "settings": {
                    "vnext": [{
                        "address": e.front.ip().to_string(),
                        "port": e.front.port(),
                        "users": [{ "id": front::USER_ID, "flow": front::FLOW, "encryption": "none" }],
                    }],
                },
                "streamSettings": {
                    "network": "raw",
                    "security": "reality",
                    "realitySettings": {
                        "serverName": front::SERVER_NAME,
                        "fingerprint": front::FINGERPRINT,
                        "publicKey": e.public_key,
                        "shortId": front::SHORT_ID,
                    },
                },
            })
        })
        .collect();
    let rules: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "type": "field",
                "inboundTag": [format!("entry-{}", e.star)],
                "outboundTag": format!("front-{}", e.star),
            })
        })
        .collect();
    json!({
        // No access log: it would name every call's destination.
        "log": { "loglevel": "warning", "access": "none" },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": { "rules": rules },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(star: u64, port: u16) -> Entry {
        Entry {
            star: StarId(star),
            port,
            front: SocketAddrV4::new([203, 0, 113, 8].into(), front::PORT),
            public_key: "DbxXuk87LHpLVR_Nj7YANVZcg9VqeUiPFDm8KajFRww".into(),
        }
    }

    #[test]
    fn each_entry_goes_to_its_own_front() {
        let c = relay_config(&[entry(8, 40001), entry(9, 40002)]);
        assert_eq!(c["inbounds"][1]["port"], 40002);
        assert_eq!(c["inbounds"][1]["listen"], "127.0.0.1");
        assert_eq!(c["routing"]["rules"][1]["inboundTag"][0], "entry-9");
        assert_eq!(c["routing"]["rules"][1]["outboundTag"], "front-9");
        let out = &c["outbounds"][0];
        assert_eq!(out["settings"]["vnext"][0]["address"], "203.0.113.8");
        assert_eq!(out["settings"]["vnext"][0]["port"], 443);
        let reality = &out["streamSettings"]["realitySettings"];
        assert_eq!(reality["serverName"], front::SERVER_NAME);
        assert_eq!(reality["shortId"], front::SHORT_ID);
        assert_eq!(c["log"]["access"], "none");
    }
}
