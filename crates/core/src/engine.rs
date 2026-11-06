//! What the supervisor asks of the half that runs the tunnel.
//!
//! On a desktop that half is `wandererd`, a privileged service the app talks
//! to over a socket; on Android it is the app's own VPN service. Both move
//! through the same three states (see [`wanderer_tunnel::ipc`]) and answer
//! with the same [`Status`].

use std::future::Future;
use std::net::Ipv4Addr;

use wanderer_tunnel::ipc::Status;
use wanderer_tunnel::{GuardPolicy, TunnelPlan};

use crate::daemon::DaemonError;

/// Runs Xray and the Network Guard for the supervisor.
pub trait Engine: Send + Sync + 'static {
    /// Where it is.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if it cannot be reached or refuses.
    fn status(&mut self) -> impl Future<Output = Result<Status, DaemonError>> + Send;

    /// Puts up the guard and starts the tunnel on `plan`.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if it cannot be reached or refuses.
    fn connect(
        &mut self,
        plan: &TunnelPlan,
        guard: &GuardPolicy,
    ) -> impl Future<Output = Result<Status, DaemonError>> + Send;

    /// Stops the tunnel and keeps the guard.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if it cannot be reached or refuses.
    fn hold(
        &mut self,
        guard: &GuardPolicy,
    ) -> impl Future<Output = Result<Status, DaemonError>> + Send;

    /// Stops the tunnel and removes the guard.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if it cannot be reached or refuses.
    fn disconnect(&mut self) -> impl Future<Output = Result<Status, DaemonError>> + Send;

    /// The IPv4 addresses of a node's `host`, resolved past the guard.
    ///
    /// # Errors
    ///
    /// Returns [`DaemonError`] if it cannot be reached or the name does not
    /// resolve.
    fn resolve(
        &mut self,
        host: &str,
    ) -> impl Future<Output = Result<Vec<Ipv4Addr>, DaemonError>> + Send;
}
