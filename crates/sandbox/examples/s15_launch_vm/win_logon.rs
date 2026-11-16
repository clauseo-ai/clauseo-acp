//! The positive control's closest variant starts its relay as a pool user the way the outer runner
//! starts the inner (`src/windows/runner/spawn.rs`, `log_on`): `CreateProcessWithLogonW` with the
//! pool user's password from `secrets.bin` (`secrets::secret_for`, as the outer reads it), its
//! profile loaded. The password is never printed; its buffer is wiped when the call returns.
//! The pool rows (`checks_pool.rs`) start their benign stand-in, `probe wait`, the same way and
//! keep it running ([`start_with_logon`]).
#![expect(
    unsafe_code,
    reason = "FFI to CreateProcessWithLogonW and the wait for its process"
)]

use std::io::BufRead as _;
use std::os::windows::io::AsRawHandle as _;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessWithLogonW, GetExitCodeProcess,
    LOGON_WITH_PROFILE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use zeroize::Zeroizing;

use super::launch::{note_ended, note_started};
use super::sys::now;
use super::win::{Owned, code_of};
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
    let (process, reader) = spawn(name, password, desktop, program, args, cwd)?;
    collect(&process, reader, limit)
}

/// A process [`start_with_logon`] started and keeps: ended on drop if it still runs.
pub(crate) struct Started {
    pid: u32,
    process: Owned,
    lines: Receiver<String>,
}

/// Starts `program args` as `name` as [`run_with_logon`] does, and returns at once with it
/// running, its stdout and stderr read line by line. Not in any job of ours.
pub(crate) fn start_with_logon(
    name: &str,
    password: Zeroizing<String>,
    desktop: &str,
    program: &Path,
    args: &[&str],
    cwd: &Path,
) -> Result<Started, String> {
    let (process, reader) = spawn(name, password, desktop, program, args, cwd)?;
    drop(Owned(process.hThread));
    let pid = process.dwProcessId;
    note_started(pid);
    let (send, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(reader).lines() {
            let Ok(line) = line else { return };
            if send.send(line.trim_end().to_owned()).is_err() {
                return;
            }
        }
    });
    Ok(Started {
        pid,
        process: Owned(process.hProcess),
        lines,
    })
}

impl Started {
    /// Its pid.
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// Its next output line, if one comes before `until`.
    pub(crate) fn line(&self, until: Instant) -> Option<String> {
        self.lines
            .recv_timeout(until.saturating_duration_since(now()))
            .ok()
    }

    /// Its exit code when it has ended or ends within `timeout`; `None` while it runs.
    pub(crate) fn exit_within(&self, timeout: Duration) -> Option<u32> {
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the process handle is live with every right (we made it).
        if unsafe { WaitForSingleObject(self.process.0, millis) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: as above; `code` is a live out-pointer.
        let read = unsafe { GetExitCodeProcess(self.process.0, &raw mut code) };
        note_ended(self.pid);
        Some(if read.is_ok() { code } else { u32::MAX })
    }

    /// Ends it if it still runs: `None` when it had ended, else `TerminateProcess`'s code (0
    /// when it ended).
    pub(crate) fn end(&self) -> Option<u32> {
        if self.exit_within(Duration::ZERO).is_some() {
            return None;
        }
        // SAFETY: the process handle is live with every right.
        let code = match unsafe { TerminateProcess(self.process.0, 1) } {
            Ok(()) => 0,
            Err(error) => code_of(&error),
        };
        // Its exit code is of no interest once it was ended here.
        let _ended = self.exit_within(Duration::from_secs(5));
        Some(code)
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // Nothing reports from a drop; the rows call `end` themselves and note it.
        let _ended = self.end();
    }
}

/// Starts `program args` as `name` (see [`run_with_logon`]); the process and the pipe its
/// stdout and stderr write to, whose write end is closed here.
fn spawn(
    name: &str,
    password: Zeroizing<String>,
    desktop: &str,
    program: &Path,
    args: &[&str],
    cwd: &Path,
) -> Result<(PROCESS_INFORMATION, std::io::PipeReader), String> {
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
    Ok((process, reader))
}
