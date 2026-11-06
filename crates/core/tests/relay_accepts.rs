//! The relay's configuration is one Xray itself accepts.
//!
//! Needs an Xray binary: `XRAY_BIN=/path/to/xray cargo test -- --ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed unwrap in a test is the test failing"
)]

use std::io::Write as _;
use std::process::{Command, Stdio};

use singula::StarId;
use wanderer_core::front::{Entry, relay_config};

#[test]
#[ignore = "needs XRAY_BIN"]
fn xray_accepts_the_relay() {
    let xray = std::env::var("XRAY_BIN").expect("XRAY_BIN");
    let entries = [8, 9].map(|star| Entry {
        star: StarId(star),
        port: 40000 + u16::try_from(star).unwrap(),
        front: "203.0.113.8:443".parse().unwrap(),
        public_key: "DbxXuk87LHpLVR_Nj7YANVZcg9VqeUiPFDm8KajFRww".into(),
    });
    let json = relay_config(&entries).to_string();
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
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
