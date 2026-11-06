//! What Wanderer's app and its privileged daemon agree on.
//!
//! The app holds the wallet, talks to Singula and decides where to connect;
//! it runs without privileges. The daemon runs as root and does only what
//! needs root: it starts Xray with a TUN interface and installs the Network
//! Guard, the firewall that keeps traffic from leaving outside the tunnel.
//!
//! The two meet over a local socket that any local user can reach, so the
//! daemon accepts only the structured [`TunnelPlan`] and [`GuardPolicy`],
//! validated field by field, and builds the Xray configuration and firewall
//! rules itself. A raw Xray configuration would let any local user make a
//! root process write files or open listeners of their choosing.
//!
//! - [`plan`]: where the tunnel goes and how, and what the guard lets through.
//! - [`xray`]: the Xray configuration for a plan.
//! - [`guard`]: the nftables ruleset for a policy (Linux).
//! - [`wfp`]: the Windows Filtering Platform filters for a policy (Windows).
//! - [`ipc`]: the requests and replies on the daemon's socket.

pub mod guard;
pub mod ipc;
pub mod plan;
pub mod wfp;
pub mod xray;

pub use plan::{
    EarlyExit, Endpoint, GuardPolicy, Hop, InvalidPlan, Reality, Transport, TunnelPlan,
};

/// The name of the TUN interface Xray creates.
///
/// Fixed, so the guard can admit it by name before it exists: Xray creates it
/// on every start, and a guard that waited for it would leave a gap.
pub const TUN_NAME: &str = "wanderer0";
