//! The TUN interface, by its name.
//!
//! Xray creates the Wintun adapter under the name the plan gives it; Windows
//! calls that its alias.

use windows_sys::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceAliasToLuid, GetIfEntry2, MIB_IF_ROW2,
};
use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;

use super::wide;

/// The interface's locally unique id, if it exists.
pub(crate) fn luid(alias: &str) -> Option<u64> {
    let alias = wide(alias);
    let mut luid = NET_LUID_LH { Value: 0 };
    // SAFETY: `alias` is NUL-terminated and outlives the call; `luid` is a
    // valid place for the result.
    let status = unsafe { ConvertInterfaceAliasToLuid(alias.as_ptr(), &raw mut luid) };
    // SAFETY: every bit pattern of the union is a valid u64.
    (status == 0).then_some(unsafe { luid.Value })
}

/// Bytes sent and received on the interface: `(out, in)`.
pub(crate) fn octets(alias: &str) -> Option<(u64, u64)> {
    let luid = luid(alias)?;
    let mut row = MIB_IF_ROW2 {
        InterfaceLuid: NET_LUID_LH { Value: luid },
        ..MIB_IF_ROW2::default()
    };
    // SAFETY: `row` is a valid MIB_IF_ROW2 with the LUID set, as GetIfEntry2
    // requires; it fills in the rest.
    let status = unsafe { GetIfEntry2(&raw mut row) };
    (status == 0).then_some((row.OutOctets, row.InOctets))
}
