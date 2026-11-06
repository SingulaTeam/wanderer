//! What the daemon needs of Windows, behind safe functions.
//!
//! The Filtering Platform, the interface table and pipe security have no
//! safe Rust bindings, so this module calls them through `windows-sys`. The
//! unsafe code stays here, each block with what it relies on; the rest of the
//! daemon sees plain functions.
//!
//! - [`wfp`]: the Network Guard as Filtering Platform filters.
//! - [`iface`]: the TUN interface, by its name.
//! - [`pipe`]: the control pipe, open to every local user.
//! - [`service`]: running as a Windows service.

#![allow(
    unsafe_code,
    reason = "Windows APIs without safe bindings; see each block"
)]

pub(crate) mod iface;
pub(crate) mod pipe;
pub(crate) mod service;
pub(crate) mod wfp;

/// `s` as a NUL-terminated UTF-16 string, as Windows takes text.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
