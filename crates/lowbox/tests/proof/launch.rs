//! The helper started as the test account: `CreateProcessWithLogonW(LOGON_WITH_PROFILE)` on the
//! private station's desktop, as the recipe's `w2launch` did, its standard handles three owner
//! pipes (the child ends inheritable). Its argv[0] is [`HELPER_ARG0`], the mark that makes the
//! re-run `lowbox_helper` test do the helper's work.
#![expect(
    unsafe_code,
    reason = "FFI to CreatePipe, SetHandleInformation, CreateProcessWithLogonW and the waits"
)]

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{
    HANDLE, HANDLE_FLAG_INHERIT, STILL_ACTIVE, SetHandleInformation, WAIT_OBJECT_0,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessWithLogonW, GetExitCodeProcess,
    LOGON_WITH_PROFILE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use super::win::{Res, failed, owned, raw, wide};

/// The helper's argv[0]: the mark `lowbox_helper` looks for.
pub(crate) const HELPER_ARG0: &str = "clauseo-lowbox-helper";

/// The started helper: its process, its input, its output lines and the whole log.
pub(crate) struct Helper {
    /// Its process id.
    pid: u32,
    /// Its process handle (all access, from `CreateProcessWithLogonW`).
    process: OwnedHandle,
    /// The owner's end of its standard input.
    stdin: File,
    /// Its standard output, line by line.
    lines: Receiver<String>,
    /// Its standard output and error, tagged, in arrival order.
    log: Arc<Mutex<Vec<String>>>,
}

/// An anonymous pipe: (read end, write end), neither inheritable.
fn pipe() -> Res<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: both outputs are locals; no attributes.
    unsafe { CreatePipe(&raw mut read, &raw mut write, None, 0) }
        .map_err(|e| failed("CreatePipe", &e))?;
    Ok((owned(read), owned(write)))
}

/// Turns `HANDLE_FLAG_INHERIT` on for a child's end.
fn inheritable(handle: &OwnedHandle) -> Res<()> {
    // SAFETY: an open handle of this process; the call only sets its flag.
    unsafe { SetHandleInformation(raw(handle), HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }
        .map_err(|e| failed("SetHandleInformation", &e))
}

/// Starts `exe` as `user` (with `password`, never printed) on `desktop`, in `exe`'s folder, with
/// the arguments that run only the `lowbox_helper` test.
pub(crate) fn start(user: &str, password: &str, exe: &Path, desktop: &str) -> Res<Helper> {
    let (in_read, in_write) = pipe()?;
    let (out_read, out_write) = pipe()?;
    let (err_read, err_write) = pipe()?;
    for child_end in [&in_read, &out_write, &err_write] {
        inheritable(child_end)?;
    }
    let mut desk = wide(desktop);
    let mut line = wide(format!(
        "{HELPER_ARG0} --exact lowbox_helper --ignored --nocapture --test-threads 1"
    ));
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or(u32::MAX),
        lpDesktop: PWSTR(desk.as_mut_ptr()),
        dwFlags: STARTF_USESTDHANDLES,
        hStdInput: raw(&in_read),
        hStdOutput: raw(&out_write),
        hStdError: raw(&err_write),
        ..STARTUPINFOW::default()
    };
    let (name, domain, secret) = (wide(user), wide("."), wide(password));
    let (app, folder) = (wide(exe), wide(exe.parent().unwrap_or(exe)));
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every string is NUL-terminated and outlives the call, the command line is
    // writable, the startup info names three open inheritable pipe ends, and `info` gets two
    // new handles, owned below.
    unsafe {
        CreateProcessWithLogonW(
            PCWSTR(name.as_ptr()),
            PCWSTR(domain.as_ptr()),
            PCWSTR(secret.as_ptr()),
            LOGON_WITH_PROFILE,
            PCWSTR(app.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            None,
            PCWSTR(folder.as_ptr()),
            &raw const startup,
            &raw mut info,
        )
    }
    .map_err(|e| failed("CreateProcessWithLogonW", &e))?;
    let process = owned(info.hProcess);
    drop(owned(info.hThread));
    drop((in_read, out_write, err_write));
    let log = Arc::new(Mutex::new(Vec::new()));
    let (sender, lines) = mpsc::channel();
    read_lines(File::from(out_read), "out", Some(sender), Arc::clone(&log));
    read_lines(File::from(err_read), "err", None, Arc::clone(&log));
    Ok(Helper {
        pid: info.dwProcessId,
        process,
        stdin: File::from(in_write),
        lines,
        log,
    })
}

/// Reads `file` line by line on a thread of its own into `log` (tagged) and `sender`.
fn read_lines(
    file: File,
    tag: &'static str,
    mut sender: Option<Sender<String>>,
    log: Arc<Mutex<Vec<String>>>,
) {
    thread::spawn(move || {
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { break };
            log.lock().unwrap().push(format!("{tag}| {line}"));
            let gone = sender.as_ref().is_some_and(|s| s.send(line).is_err());
            if gone {
                sender = None;
            }
        }
    });
}

impl Helper {
    /// Its process id.
    pub(crate) fn pid(&self) -> u32 {
        self.pid
    }

    /// Its process handle.
    pub(crate) fn process(&self) -> BorrowedHandle<'_> {
        self.process.as_handle()
    }

    /// Writes `line` and a newline to its standard input.
    pub(crate) fn send(&mut self, line: &str) -> Res<()> {
        writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("writing to the helper's input: {e}"))
    }

    /// Its next output line, waiting at most `timeout`.
    pub(crate) fn next_line(&self, timeout: Duration) -> Result<String, RecvTimeoutError> {
        self.lines.recv_timeout(timeout)
    }

    /// A copy of the log so far.
    pub(crate) fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    /// Waits at most `ms` for it to exit; whether it did.
    pub(crate) fn wait(&self, ms: u32) -> bool {
        // SAFETY: an open process handle.
        let event = unsafe { WaitForSingleObject(raw(&self.process), ms) };
        event == WAIT_OBJECT_0
    }

    /// Its exit code, or `None` while it runs.
    pub(crate) fn exit_code(&self) -> Option<u32> {
        let mut code = 0_u32;
        // SAFETY: an open process handle and a local.
        let read = unsafe { GetExitCodeProcess(raw(&self.process), &raw mut code) };
        (read.is_ok() && code != STILL_ACTIVE.0.cast_unsigned()).then_some(code)
    }

    /// Ends it if it still runs (its Job Object then ends chrome), and waits for the exit.
    pub(crate) fn kill(self) -> Res<String> {
        if let Some(code) = self.exit_code() {
            return Ok(format!(
                "the helper had exited with {code:#x}; nothing to kill"
            ));
        }
        // SAFETY: an open process handle with PROCESS_TERMINATE.
        unsafe { TerminateProcess(raw(&self.process), 0xDEAD) }
            .map_err(|e| failed("TerminateProcess(helper)", &e))?;
        if !self.wait(10_000) {
            return Err("the helper did not exit within 10 s of TerminateProcess".to_owned());
        }
        Ok("terminated the helper (its Job Object ends chrome)".to_owned())
    }
}
