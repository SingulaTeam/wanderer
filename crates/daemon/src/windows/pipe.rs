//! The control pipe, open to every local user.
//!
//! A named pipe's default security lets only administrators and the creator
//! write to it, and the app runs as an ordinary user. So each instance is
//! created with a descriptor of its own: full access for the system and
//! administrators, read and write for any authenticated user — what the
//! socket's mode 0666 is on Linux. What a request may do is limited by its
//! type, not by who sends it (see `server`).

use std::ffi::c_void;
use std::io;
use std::path::Path;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

use super::wide;

/// System and administrators: all; authenticated users: read and write.
const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)";

/// Creates the next instance of the pipe at `path`. The first one fails if
/// the pipe exists already — another program holding the name would be
/// answering the app in the daemon's place.
pub(crate) fn create(path: &Path, first: bool) -> io::Result<NamedPipeServer> {
    let sddl = wide(SDDL);
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl` is NUL-terminated; on success Windows allocates the
    // descriptor, which is freed below with LocalFree.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &raw mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor
    // lives until LocalFree below, after the pipe is created.
    let pipe = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .create_with_security_attributes_raw(path, (&raw mut attributes).cast::<c_void>())
    };
    // SAFETY: `descriptor` came from the conversion above and is freed once.
    unsafe { LocalFree(descriptor as HLOCAL) };
    pipe
}
