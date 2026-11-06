//! The guard's scripts are ones nftables itself accepts, applied for real in
//! a throwaway user and network namespace, so no root is needed.
//!
//! Needs `nft` and `unshare`: `cargo test -- --ignored`.

#![allow(
    clippy::unwrap_used,
    reason = "a failed unwrap in a test is the test failing"
)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::{Command, Stdio};

use wanderer_tunnel::guard::{install, remove};
use wanderer_tunnel::{Endpoint, GuardPolicy};

/// Runs the scripts one after another in one fresh namespace, as the daemon
/// would over a connect, a reconnect and a disconnect.
fn apply(scripts: &[String]) -> std::process::Output {
    let mut shell = String::new();
    for (i, script) in scripts.iter().enumerate() {
        let _ = writeln!(shell, "nft -f - <<'S{i}' || exit 1\n{script}S{i}");
    }
    shell.push_str("nft list tables\n");
    let mut child = Command::new("unshare")
        .args(["-rn", "sh"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(shell.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
#[ignore = "needs nft and unprivileged user namespaces"]
fn nftables_accepts_install_replace_and_remove() {
    let cp = Endpoint("198.51.100.4:443".parse().unwrap());
    let node = Some("203.0.113.7:443".parse().unwrap());
    let open = GuardPolicy {
        allowed: vec![cp],
        allow_lan: true,
    };

    let out = apply(&[
        remove(),
        install(&GuardPolicy::default(), None),
        install(&open, node),
        install(&open, None),
    ]);
    let tables = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tables.contains("table inet wanderer"), "{tables}");

    let out = apply(&[install(&open, node), remove()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).contains("wanderer"));
}
