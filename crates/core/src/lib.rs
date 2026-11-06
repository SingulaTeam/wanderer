//! Wanderer's core: everything the app does that needs no privileges.
//!
//! It opens Singula sessions, chooses how to reach each node, and keeps the
//! tunnel up through an [`Engine`](engine::Engine), which does the privileged
//! half — Xray and the Network Guard: the daemon on a desktop, the VPN
//! service on Android. The UI only shows [`supervisor::State`] and sends
//! commands through a [`supervisor::Handle`].
//!
//! - [`engine`]: what the supervisor asks of the half that runs the tunnel.
//! - [`daemon`]: the socket to `wandererd`.
//! - [`front`]: the relay to Control Planes' fronts, where they are blocked.
//! - [`reach`]: the chain and the Control Planes, at addresses resolved
//!   before the guard blocks DNS.
//! - [`route`]: which nodes a chain runs through.
//! - [`transport`]: Vision or xHTTP, on which cover, in which order.
//! - [`supervisor`]: the connection's state machine.
//! - [`problem`]: what went wrong, in terms the person can act on.

pub mod daemon;
pub mod engine;
pub mod front;
pub mod problem;
pub mod reach;
pub mod route;
pub mod supervisor;
pub mod transport;
