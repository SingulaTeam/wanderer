//! A chain built by [`config`] carries traffic through every hop, and an
//! early exit leaves at its hop.
//!
//! Three local Xray servers stand in for nodes: the first serves Vision, as a
//! chain's entry does, the others serve flowless legs. The client is the
//! configuration the daemon would run, with a SOCKS inbound in place of the
//! TUN — it keeps the TUN's tag, so routing is the same.
//!
//! Needs an Xray binary and the network, since REALITY borrows a real site's
//! handshake: `XRAY_BIN=/path/to/xray cargo test -- --ignored`.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is the test failing"
)]

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use wanderer_tunnel::xray::{Host, config};
use wanderer_tunnel::{EarlyExit, Hop, Reality, Transport, TunnelPlan};

const PRIVATE_KEY: &str = "ULw3MVRstZBmlk2V7etFFtrHI7-L4WDxB2Ig0dlUfF0";
const PUBLIC_KEY: &str = "RoVb1JP4IfCan5Wv47SJKYtXkSulY_esacbfRPlTP2I";
const SERVER_NAME: &str = "www.adobe.com";
const SHORT_ID: &str = "a1b2c3d4";
/// Where the test's traffic goes, and where its early exit goes.
const THROUGH: &str = "1.1.1.1";
const EARLY: &str = "1.0.0.1";

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn run(xray: &str, config: &Value) -> Running {
    let mut child = Command::new(xray)
        .args(["run", "-format", "json", "-c", "stdin:"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(if std::env::var_os("CHAIN_DEBUG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(config.to_string().as_bytes())
        .unwrap();
    Running(child)
}

/// A node: VLESS over REALITY, out to anywhere, logging where each
/// connection asked to go.
fn node(port: u16, uuid: &str, flow: &str, log: &Path) -> Value {
    json!({
        "log": { "loglevel": "warning", "access": log },
        "inbounds": [{
            "listen": "127.0.0.1",
            "port": port,
            "protocol": "vless",
            "settings": { "clients": [{ "id": uuid, "flow": flow }], "decryption": "none" },
            "streamSettings": {
                "network": "raw",
                "security": "reality",
                "realitySettings": {
                    "dest": format!("{SERVER_NAME}:443"),
                    "serverNames": [SERVER_NAME],
                    "privateKey": PRIVATE_KEY,
                    "shortIds": [SHORT_ID],
                },
            },
        }],
        // Freedom refuses private addresses unless told otherwise, and the
        // next hop here is on loopback.
        "outbounds": [{
            "protocol": "freedom",
            "settings": { "finalRules": [{ "action": "allow", "ip": ["127.0.0.0/8"] }] },
        }],
    })
}

fn reality() -> Reality {
    Reality {
        public_key: PUBLIC_KEY.into(),
        short_id: SHORT_ID.into(),
        server_name: SERVER_NAME.into(),
        fingerprint: "chrome".into(),
    }
}

fn uuid(i: usize) -> String {
    format!("0b3f5a2e-1c4d-4e6f-8a9b-{i:012x}")
}

/// An HTTP request to `host` port 80 through the SOCKS entry; the status line.
fn get(entry: u16, host: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", entry)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let ip: Ipv4Addr = host.parse().unwrap();
    s.write_all(&[5, 1, 0]).unwrap();
    let mut reply = [0; 2];
    s.read_exact(&mut reply).unwrap();
    let mut connect = vec![5, 1, 0, 1];
    connect.extend(ip.octets());
    connect.extend(80u16.to_be_bytes());
    s.write_all(&connect).unwrap();
    let mut reply = [0; 10];
    s.read_exact(&mut reply).unwrap();
    assert_eq!(reply[1], 0, "SOCKS connect to {host} refused");
    s.write_all(format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut answer = Vec::new();
    let _ = s.read_to_end(&mut answer);
    String::from_utf8_lossy(&answer)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn wait_for(port: u16) {
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "port {port} never opened"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "needs XRAY_BIN and the network"]
fn a_chain_carries_traffic_and_early_exits_leave_at_their_hop() {
    let xray = std::env::var("XRAY_BIN").expect("XRAY_BIN");
    let dir = std::env::temp_dir().join(format!("wanderer-chain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let logs: Vec<_> = (0..3).map(|i| dir.join(format!("node{i}.log"))).collect();
    let ports: Vec<u16> = (0..3).map(|_| free_port()).collect();

    let _nodes: Vec<Running> = (0..3)
        .map(|i| {
            let flow = if i == 0 { "xtls-rprx-vision" } else { "" };
            run(&xray, &node(ports[i], &uuid(i), flow, &logs[i]))
        })
        .collect();

    let at = |i: usize| SocketAddrV4::new(Ipv4Addr::LOCALHOST, ports[i]);
    let plan = TunnelPlan {
        node: at(0),
        uuid: uuid(0),
        reality: reality(),
        transport: Transport::Vision,
        onward: (1..3)
            .map(|i| Hop {
                node: at(i),
                uuid: uuid(i),
                reality: reality(),
            })
            .collect(),
        early_exits: vec![EarlyExit {
            hop: 0,
            address: EARLY.parse().unwrap(),
        }],
    };
    let mut client = config(&plan, Host::Android);
    let entry = free_port();
    client["inbounds"] = json!([{
        "tag": "tun",
        "listen": "127.0.0.1",
        "port": entry,
        "protocol": "socks",
        "settings": { "udp": false },
    }]);
    let _client = run(&xray, &client);
    for &port in ports.iter().chain([&entry]) {
        wait_for(port);
    }

    assert!(
        get(entry, THROUGH).starts_with("HTTP/1.1"),
        "nothing came back through the chain"
    );
    assert!(
        get(entry, EARLY).starts_with("HTTP/1.1"),
        "nothing came back from the early exit"
    );
    // Access logs are written as connections close.
    std::thread::sleep(Duration::from_secs(1));

    let log = |i: usize| std::fs::read_to_string(&logs[i]).unwrap_or_default();
    let (first, middle, last) = (log(0), log(1), log(2));
    let next = |i: usize| format!("127.0.0.1:{}", ports[i]);
    assert!(
        first.contains(&next(1)),
        "the entry did not carry the leg:\n{first}"
    );
    assert!(
        middle.contains(&next(2)),
        "the middle did not carry the leg:\n{middle}"
    );
    assert!(
        last.contains(&format!("{THROUGH}:80")),
        "the exit did not reach the site:\n{last}"
    );
    assert!(
        !first.contains(&format!("{THROUGH}:80")),
        "the entry saw the destination:\n{first}"
    );
    assert!(
        first.contains(&format!("{EARLY}:80")),
        "the early exit did not leave at the entry:\n{first}"
    );
    assert!(
        !last.contains(EARLY),
        "the early exit went all the way:\n{last}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
