//! The configurations are ones Xray itself accepts.
//!
//! Needs an Xray binary: `XRAY_BIN=/path/to/xray cargo test -- --ignored`.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is the test failing"
)]

use std::io::Write as _;
use std::process::{Command, Stdio};

use wanderer_tunnel::xray::{Host, config};
use wanderer_tunnel::{EarlyExit, Hop, Reality, Transport, TunnelPlan};

fn plan(transport: Transport) -> TunnelPlan {
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

/// Three nodes, with a Control Plane's address leaving at each of the first
/// two.
fn chain(transport: Transport) -> TunnelPlan {
    let mut p = plan(transport);
    for (i, node) in ["198.51.100.10:443", "198.51.100.11:443"]
        .iter()
        .enumerate()
    {
        p.onward.push(Hop {
            node: node.parse().unwrap(),
            uuid: "0b3f5a2e-1c4d-4e6f-8a9b-000000000001".into(),
            reality: p.reality.clone(),
        });
        p.early_exits.push(EarlyExit {
            hop: u8::try_from(i).unwrap(),
            address: "203.0.113.50".parse().unwrap(),
        });
    }
    p
}

#[test]
#[ignore = "needs XRAY_BIN"]
fn xray_accepts_both_transports() {
    let xray = std::env::var("XRAY_BIN").expect("XRAY_BIN");
    for transport in [
        Transport::Vision,
        Transport::Xhttp {
            path: "/api/v1/session/heartbeat".into(),
        },
    ] {
        for plan in [plan(transport.clone()), chain(transport.clone())] {
            // Windows' TUN settings are checked only on Windows, where its
            // filters exist.
            for host in [
                Host::Linux {
                    systemd_resolved: true,
                },
                Host::Linux {
                    systemd_resolved: false,
                },
                Host::Android,
            ] {
                let json = config(&plan, host).to_string();
                // The way the daemon hands it over: on stdin, never in a file.
                let mut child = Command::new(&xray)
                    .args(["run", "-test", "-format", "json", "-c", "stdin:"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(json.as_bytes())
                    .unwrap();
                let out = child.wait_with_output().unwrap();
                assert!(
                    out.status.success(),
                    "{transport:?}: {}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
}
