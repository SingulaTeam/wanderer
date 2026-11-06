//! The Network Guard as Windows Filtering Platform filters.
//!
//! The filters are [`wanderer_tunnel::wfp::filters`], added in a sublayer
//! the daemon owns. Each one gets a key made from a fixed base and its
//! position, so the daemon finds its filters by key alone — after a restart
//! as well, when it no longer knows what it added. Replacing the guard
//! deletes every such key and adds the new filters in one transaction: there
//! is no moment without rules, as with `nft -f`.
//!
//! The session is not dynamic, so the filters outlive the daemon until the
//! next boot: a daemon that dies while connected leaves the machine offline,
//! not leaking, and removes them when it starts again.

use std::ffi::c_void;

use wanderer_tunnel::TUN_NAME;
use wanderer_tunnel::wfp::{Action, Condition, Family, Filter};
use windows_sys::Win32::Foundation::{FWP_E_ALREADY_EXISTS, FWP_E_FILTER_NOT_FOUND, HANDLE};
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_BYTE_BLOB, FWP_BYTE_BLOB_TYPE,
    FWP_CONDITION_FLAG_IS_LOOPBACK, FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL,
    FWP_MATCH_FLAGS_ALL_SET, FWP_UINT8, FWP_UINT16, FWP_UINT32, FWP_UINT64, FWP_V4_ADDR_AND_MASK,
    FWP_V4_ADDR_MASK, FWP_VALUE0, FWP_VALUE0_0, FWPM_ACTION0, FWPM_CONDITION_ALE_APP_ID,
    FWPM_CONDITION_FLAGS, FWPM_CONDITION_IP_LOCAL_INTERFACE, FWPM_CONDITION_IP_LOCAL_PORT,
    FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS, FWPM_CONDITION_IP_REMOTE_PORT,
    FWPM_DISPLAY_DATA0, FWPM_FILTER_CONDITION0, FWPM_FILTER_FLAG_NONE, FWPM_FILTER0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_PROVIDER0, FWPM_SUBLAYER0,
    FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0, FwpmFilterDeleteByKey0, FwpmFreeMemory0,
    FwpmGetAppIdFromFileName0, FwpmProviderAdd0, FwpmSubLayerAdd0, FwpmTransactionAbort0,
    FwpmTransactionBegin0, FwpmTransactionCommit0,
};
use windows_sys::Win32::System::Rpc::RPC_C_AUTHN_WINNT;
use windows_sys::core::GUID;

use super::{iface, wide};

/// Wanderer's WFP provider: who owns the filters, for anyone listing them.
const PROVIDER: GUID = GUID::from_u128(0x7761_6e64_6572_6572_8f3a_5c1e_0d2b_4a01);
/// The sublayer the guard's filters are weighed in.
const SUBLAYER: GUID = GUID::from_u128(0x7761_6e64_6572_6572_8f3a_5c1e_0d2b_4a02);
/// Filter keys are this base plus a filter's position.
const FILTER_BASE: u128 = 0x7761_6e64_6572_6572_8f3a_5c1e_0d2b_0000;
/// More keys than the guard ever adds: 32 allowed endpoints and a dozen of
/// its own.
const MAX_FILTERS: u16 = 64;

/// Why the guard could not be changed: the call and its error code.
#[derive(Debug, thiserror::Error)]
#[error("{call} failed with {code:#010x}")]
pub(crate) struct WfpError {
    call: &'static str,
    code: u32,
}

fn check(call: &'static str, code: u32) -> Result<(), WfpError> {
    if code == 0 {
        Ok(())
    } else {
        Err(WfpError { call, code })
    }
}

/// An open session with the filter engine, closed when dropped.
struct Engine(HANDLE);

impl Engine {
    fn open() -> Result<Self, WfpError> {
        let mut handle: HANDLE = std::ptr::null_mut();
        // SAFETY: a null server name and session ask for the local engine and
        // a default, non-dynamic session; `handle` receives the result.
        let code = unsafe {
            FwpmEngineOpen0(
                std::ptr::null(),
                RPC_C_AUTHN_WINNT,
                std::ptr::null(),
                std::ptr::null(),
                &raw mut handle,
            )
        };
        check("FwpmEngineOpen0", code)?;
        Ok(Self(handle))
    }

    /// Runs `f` in a transaction, committed only if it succeeds.
    fn transaction(&self, f: impl FnOnce(&Self) -> Result<(), WfpError>) -> Result<(), WfpError> {
        // SAFETY: the handle is open for the life of `self`.
        check("FwpmTransactionBegin0", unsafe {
            FwpmTransactionBegin0(self.0, 0)
        })?;
        match f(self) {
            // SAFETY: as above; a transaction is open.
            Ok(()) => check("FwpmTransactionCommit0", unsafe {
                FwpmTransactionCommit0(self.0)
            }),
            Err(e) => {
                // SAFETY: as above; nothing of the failed changes is kept.
                unsafe { FwpmTransactionAbort0(self.0) };
                Err(e)
            }
        }
    }

    /// Adds the provider and the sublayer, unless they exist already.
    fn register(&self) -> Result<(), WfpError> {
        let mut name = wide("Wanderer");
        let mut description = wide("Wanderer's Network Guard: nothing leaves outside the tunnel.");
        let display = FWPM_DISPLAY_DATA0 {
            name: name.as_mut_ptr(),
            description: description.as_mut_ptr(),
        };
        let provider = FWPM_PROVIDER0 {
            providerKey: PROVIDER,
            displayData: display,
            ..FWPM_PROVIDER0::default()
        };
        // SAFETY: `provider` and the strings it points to live past the call.
        let code = unsafe { FwpmProviderAdd0(self.0, &raw const provider, std::ptr::null_mut()) };
        exists_ok("FwpmProviderAdd0", code)?;

        let mut provider_key = PROVIDER;
        let sublayer = FWPM_SUBLAYER0 {
            subLayerKey: SUBLAYER,
            displayData: display,
            providerKey: &raw mut provider_key,
            weight: u16::MAX,
            ..FWPM_SUBLAYER0::default()
        };
        // SAFETY: as above, for `sublayer` and the key it points to.
        let code = unsafe { FwpmSubLayerAdd0(self.0, &raw const sublayer, std::ptr::null_mut()) };
        exists_ok("FwpmSubLayerAdd0", code)
    }

    /// Deletes every filter the guard may have added.
    fn clear(&self) -> Result<(), WfpError> {
        for i in 0..MAX_FILTERS {
            let key = key(i);
            // SAFETY: `key` lives past the call.
            let code = unsafe { FwpmFilterDeleteByKey0(self.0, &raw const key) };
            if code != FWP_E_FILTER_NOT_FOUND.cast_unsigned() {
                check("FwpmFilterDeleteByKey0", code)?;
            }
        }
        Ok(())
    }

    fn add(&self, index: u16, filter: &Filter, context: &Context) -> Result<(), WfpError> {
        // Everything the conditions point to lives in `values` until the
        // filter is added.
        let mut values = Values::default();
        let mut conditions: Vec<FWPM_FILTER_CONDITION0> = Vec::new();
        for condition in &filter.conditions {
            match values.condition(condition, context) {
                Some(c) => conditions.push(c),
                // The TUN without an interface: nothing to admit yet.
                None => return Ok(()),
            }
        }
        let mut name = wide(filter.name);
        let mut provider_key = PROVIDER;
        let raw = FWPM_FILTER0 {
            filterKey: key(index),
            displayData: FWPM_DISPLAY_DATA0 {
                name: name.as_mut_ptr(),
                description: std::ptr::null_mut(),
            },
            flags: FWPM_FILTER_FLAG_NONE,
            providerKey: &raw mut provider_key,
            layerKey: match filter.family {
                Family::V4 => FWPM_LAYER_ALE_AUTH_CONNECT_V4,
                Family::V6 => FWPM_LAYER_ALE_AUTH_CONNECT_V6,
            },
            subLayerKey: SUBLAYER,
            weight: FWP_VALUE0 {
                r#type: FWP_UINT8,
                Anonymous: FWP_VALUE0_0 {
                    uint8: filter.weight,
                },
            },
            numFilterConditions: u32::try_from(conditions.len()).unwrap_or(0),
            filterCondition: if conditions.is_empty() {
                std::ptr::null_mut()
            } else {
                conditions.as_mut_ptr()
            },
            action: FWPM_ACTION0 {
                r#type: match filter.action {
                    Action::Permit => FWP_ACTION_PERMIT,
                    Action::Block => FWP_ACTION_BLOCK,
                },
                ..FWPM_ACTION0::default()
            },
            ..FWPM_FILTER0::default()
        };
        let mut id = 0u64;
        // SAFETY: `raw`, its conditions, the values they point to, the name
        // and the provider key all live past the call.
        let code =
            unsafe { FwpmFilterAdd0(self.0, &raw const raw, std::ptr::null_mut(), &raw mut id) };
        check("FwpmFilterAdd0", code)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the handle was opened in `open` and is closed once.
        unsafe { FwpmEngineClose0(self.0) };
    }
}

fn exists_ok(call: &'static str, code: u32) -> Result<(), WfpError> {
    if code == FWP_E_ALREADY_EXISTS.cast_unsigned() {
        Ok(())
    } else {
        check(call, code)
    }
}

fn key(index: u16) -> GUID {
    GUID::from_u128(FILTER_BASE | u128::from(index))
}

/// What the conditions refer to on this machine.
struct Context {
    /// The TUN's LUID, if it exists.
    tun: Option<u64>,
    /// The daemon's application id, as WFP identifies a program.
    daemon: AppId,
}

/// Storage the conditions point into.
///
/// Boxed: the conditions hold pointers to these values, and a vector moves
/// its elements when it grows.
#[derive(Default)]
#[allow(clippy::vec_box, reason = "the values must not move once pointed to")]
struct Values {
    u64s: Vec<Box<u64>>,
    masks: Vec<Box<FWP_V4_ADDR_AND_MASK>>,
}

impl Values {
    /// `condition` as WFP takes it; `None` for the TUN when there is none.
    fn condition(
        &mut self,
        condition: &Condition,
        context: &Context,
    ) -> Option<FWPM_FILTER_CONDITION0> {
        let (field, matching, value) = match condition {
            Condition::Loopback => (
                FWPM_CONDITION_FLAGS,
                FWP_MATCH_FLAGS_ALL_SET,
                value(
                    FWP_UINT32,
                    FWP_CONDITION_VALUE0_0 {
                        uint32: FWP_CONDITION_FLAG_IS_LOOPBACK,
                    },
                ),
            ),
            Condition::Tun => {
                let mut luid = Box::new(context.tun?);
                let pointer = &raw mut *luid;
                self.u64s.push(luid);
                (
                    FWPM_CONDITION_IP_LOCAL_INTERFACE,
                    FWP_MATCH_EQUAL,
                    value(FWP_UINT64, FWP_CONDITION_VALUE0_0 { uint64: pointer }),
                )
            }
            Condition::Daemon => (
                FWPM_CONDITION_ALE_APP_ID,
                FWP_MATCH_EQUAL,
                value(
                    FWP_BYTE_BLOB_TYPE,
                    FWP_CONDITION_VALUE0_0 {
                        byteBlob: context.daemon.0,
                    },
                ),
            ),
            Condition::RemoteNetwork { address, prefix } => {
                let mask = u32::MAX.checked_shl(32 - u32::from(*prefix)).unwrap_or(0);
                // Host byte order, as WFP takes IPv4 addresses.
                let mut m = Box::new(FWP_V4_ADDR_AND_MASK {
                    addr: u32::from(*address),
                    mask,
                });
                let pointer = &raw mut *m;
                self.masks.push(m);
                (
                    FWPM_CONDITION_IP_REMOTE_ADDRESS,
                    FWP_MATCH_EQUAL,
                    value(
                        FWP_V4_ADDR_MASK,
                        FWP_CONDITION_VALUE0_0 {
                            v4AddrMask: pointer,
                        },
                    ),
                )
            }
            Condition::RemotePort(port) => (
                FWPM_CONDITION_IP_REMOTE_PORT,
                FWP_MATCH_EQUAL,
                value(FWP_UINT16, FWP_CONDITION_VALUE0_0 { uint16: *port }),
            ),
            Condition::LocalPort(port) => (
                FWPM_CONDITION_IP_LOCAL_PORT,
                FWP_MATCH_EQUAL,
                value(FWP_UINT16, FWP_CONDITION_VALUE0_0 { uint16: *port }),
            ),
            Condition::Protocol(protocol) => (
                FWPM_CONDITION_IP_PROTOCOL,
                FWP_MATCH_EQUAL,
                value(
                    FWP_UINT8,
                    FWP_CONDITION_VALUE0_0 {
                        uint8: protocol.number(),
                    },
                ),
            ),
        };
        Some(FWPM_FILTER_CONDITION0 {
            fieldKey: field,
            matchType: matching,
            conditionValue: value,
        })
    }
}

fn value(
    kind: windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::FWP_DATA_TYPE,
    anonymous: FWP_CONDITION_VALUE0_0,
) -> FWP_CONDITION_VALUE0 {
    FWP_CONDITION_VALUE0 {
        r#type: kind,
        Anonymous: anonymous,
    }
}

/// A program's WFP application id, freed when dropped.
struct AppId(*mut FWP_BYTE_BLOB);

impl AppId {
    /// The id of the running daemon's executable.
    fn of_this_program() -> Result<Self, WfpError> {
        let path = std::env::current_exe().map_err(|_| WfpError {
            call: "current_exe",
            code: 0,
        })?;
        let path = wide(&path.to_string_lossy());
        let mut blob: *mut FWP_BYTE_BLOB = std::ptr::null_mut();
        // SAFETY: `path` is NUL-terminated; WFP allocates the blob, freed in
        // Drop with FwpmFreeMemory0.
        let code = unsafe { FwpmGetAppIdFromFileName0(path.as_ptr(), &raw mut blob) };
        check("FwpmGetAppIdFromFileName0", code)?;
        Ok(Self(blob))
    }
}

impl Drop for AppId {
    fn drop(&mut self) {
        let mut pointer = self.0.cast::<c_void>();
        // SAFETY: the blob was allocated by WFP and is freed once.
        unsafe { FwpmFreeMemory0(&raw mut pointer) };
    }
}

/// Replaces the guard with `filters`. The TUN is admitted if it exists.
pub(crate) fn apply(filters: &[Filter]) -> Result<(), WfpError> {
    let engine = Engine::open()?;
    engine.register()?;
    let context = Context {
        tun: iface::luid(TUN_NAME),
        daemon: AppId::of_this_program()?,
    };
    engine.transaction(|e| {
        e.clear()?;
        for (index, filter) in (0u16..).zip(filters) {
            e.add(index, filter, &context)?;
        }
        Ok(())
    })
}

/// Removes the guard; harmless when there is none.
pub(crate) fn remove() -> Result<(), WfpError> {
    let engine = Engine::open()?;
    engine.transaction(Engine::clear)
}
