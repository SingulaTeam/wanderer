//! Installing and removing the Network Guard on this system.
//!
//! Linux applies [`wanderer_tunnel::guard`]'s ruleset with `nft`; Windows
//! adds [`wanderer_tunnel::wfp`]'s filters through the Filtering Platform.

use std::net::SocketAddrV4;

use wanderer_tunnel::GuardPolicy;

/// Installs the guard for `policy`, admitting `node` as well, replacing any
/// guard there is.
///
/// `tun` says the TUN exists. Linux admits it by name whether it exists or
/// not; Windows admits the interface itself, so the daemon installs the guard
/// again once Xray has created it.
#[cfg(unix)]
pub(crate) async fn install(
    policy: &GuardPolicy,
    node: Option<SocketAddrV4>,
    _tun: bool,
) -> Result<(), String> {
    crate::nft::apply(&wanderer_tunnel::guard::install(policy, node))
        .await
        .map_err(|e| e.to_string())
}

/// Removes the guard; harmless when there is none.
#[cfg(unix)]
pub(crate) async fn remove() -> Result<(), String> {
    crate::nft::apply(&wanderer_tunnel::guard::remove())
        .await
        .map_err(|e| e.to_string())
}

/// Installs the guard for `policy`, admitting `node` as well, replacing any
/// guard there is.
///
/// `tun` says the TUN exists. Linux admits it by name whether it exists or
/// not; Windows admits the interface itself, so the daemon installs the guard
/// again once Xray has created it.
#[cfg(windows)]
pub(crate) async fn install(
    policy: &GuardPolicy,
    node: Option<SocketAddrV4>,
    tun: bool,
) -> Result<(), String> {
    let filters = wanderer_tunnel::wfp::filters(policy, node, tun);
    blocking(move || crate::windows::wfp::apply(&filters)).await
}

/// Removes the guard; harmless when there is none.
#[cfg(windows)]
pub(crate) async fn remove() -> Result<(), String> {
    blocking(crate::windows::wfp::remove).await
}

#[cfg(windows)]
async fn blocking(
    f: impl FnOnce() -> Result<(), crate::windows::wfp::WfpError> + Send + 'static,
) -> Result<(), String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Whether installing the guard again once the TUN exists changes it.
pub(crate) const ADMITS_TUN_BY_INTERFACE: bool = cfg!(windows);
