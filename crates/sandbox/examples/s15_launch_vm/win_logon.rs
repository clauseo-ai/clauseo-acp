//! The positive control's closest variant starts its relay as a pool user the way the outer runner
//! starts the inner (`src/windows/runner/spawn.rs`, `log_on`): `CreateProcessWithLogonW` with the
//! pool user's password from `secrets.bin` (`secrets::secret_for`, as the outer reads it), its
//! profile loaded. The password is never printed; its buffer is wiped when the call returns.
#![expect(unsafe_code, reason = "FFI to CreateProcessWithLogonW")]

use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessWithLogonW, LOGON_WITH_PROFILE,
    PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use super::win::code_of;
use super::win_ctl::{Ran, collect};

/// Starts `program args` as the local user `name`, logged on with `password` (profile loaded,
/// the user's own environment), on `desktop`, in `cwd`, its stdout and stderr one pipe; waits up
/// to `limit`, then ends it. Not in any job of ours. The call that failed and its code otherwise.
pub(crate) fn run_with_logon(
    name: &str,
    password: Zeroizing<String>,
    desktop: &str,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    limit: Duration,
) -> Result<Ran, String> {
    let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(password.len() + 1));
    secret.extend(password.encode_utf16());
    secret.push(0);
    drop(password);
    let (reader, writer) = std::io::pipe().map_err(|e| format!("CreatePipe: {e}"))?;
    let write = HANDLE(writer.as_raw_handle());
    // SAFETY: `write` is the live write end `writer` owns.
    unsafe { SetHandleInformation(write, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }
        .map_err(|e| format!("SetHandleInformation gave {}", code_of(&e)))?;
    let quoted = args.iter().map(|arg| format!(" \"{arg}\""));
    let line = format!("\"{}\"{}", program.display(), quoted.collect::<String>());
    let mut line: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
    let mut desk: Vec<u16> = desktop.encode_utf16().chain(std::iter::once(0)).collect();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(0),
        lpDesktop: PWSTR(desk.as_mut_ptr()),
        dwFlags: STARTF_USESTDHANDLES,
        hStdOutput: write,
        hStdError: write,
        ..STARTUPINFOW::default()
    };
    let mut process = PROCESS_INFORMATION::default();
    // SAFETY: every string is NUL-terminated and outlives the call, the command line and desktop
    // writable; no environment, so the user's own is made from its profile; the pipe handle in
    // `startup` is live until `writer` drops below; `process` is a live out-pointer whose handles
    // `collect` owns.
    let started = unsafe {
        CreateProcessWithLogonW(
            &HSTRING::from(name),
            &HSTRING::from("."),
            PCWSTR(secret.as_ptr()),
            LOGON_WITH_PROFILE,
            PCWSTR::null(),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            None,
            &HSTRING::from(cwd.as_os_str()),
            &raw const startup,
            &raw mut process,
        )
    };
    drop(secret);
    drop(writer);
    started.map_err(|e| format!("CreateProcessWithLogonW({name}) gave {}", code_of(&e)))?;
    collect(&process, reader, limit)
}
