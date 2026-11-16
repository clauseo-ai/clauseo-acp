//! A process an elevated test starts as its test user, with the user's profile loaded
//! (`LOGON_WITH_PROFILE`), through the Secondary Logon service: what the user does in its own
//! profile, done by the user rather than by the elevated test.
#![expect(
    unsafe_code,
    reason = "FFI to start a process as the test user and to read its exit code"
)]

use std::iter::once;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessWithLogonW, GetExitCodeProcess, LOGON_WITH_PROFILE,
    PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};
use zeroize::Zeroizing;

use super::TestUser;

/// How long a command run by [`run_in_cmd`] may take, in milliseconds.
const COMMAND_MS: u32 = 120_000;
/// The file in the profile a command's output goes to, read back when it fails.
const OUTPUT: &str = "clauseo-test-output.txt";

/// `%SystemRoot%\System32`.
fn system32() -> String {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    format!(r"{system}\System32")
}

/// A process this test started as the test user; ended (its handle alone) if still running,
/// and waited for, on drop, so a failed assertion still stops it.
pub(super) struct Running(PROCESS_INFORMATION);

impl Running {
    /// `ping` for five minutes as `user`: a process that keeps the user's profile loaded.
    pub(super) fn ping(user: &TestUser) -> Running {
        let ping = format!(r"{}\PING.EXE", system32());
        Running::start(user, &ping, &format!(r#""{ping}" -n 300 127.0.0.1"#))
    }

    /// `application` with the command line `command` as `user`, its profile loaded, with no
    /// window. The password goes in a buffer wiped on drop.
    fn start(user: &TestUser, application: &str, command: &str) -> Running {
        let folder = HSTRING::from(system32());
        let application = HSTRING::from(application);
        let mut command: Vec<u16> = command.encode_utf16().chain(once(0)).collect();
        let password: Zeroizing<Vec<u16>> =
            Zeroizing::new(user.password.encode_utf16().chain(once(0)).collect());
        let startup = STARTUPINFOW {
            cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap(),
            ..STARTUPINFOW::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: every string is NUL-terminated and lives for the call; the command line is a
        // mutable buffer, as the call needs; `startup` and `info` are live.
        let started = unsafe {
            CreateProcessWithLogonW(
                &HSTRING::from(user.name.as_str()),
                w!("."),
                PCWSTR(password.as_ptr()),
                LOGON_WITH_PROFILE,
                &application,
                Some(PWSTR(command.as_mut_ptr())),
                CREATE_NO_WINDOW,
                None,
                &folder,
                &raw const startup,
                &raw mut info,
            )
        };
        if let Err(error) = started {
            panic!("CreateProcessWithLogonW (needs the Secondary Logon service): {error}");
        }
        Running(info)
    }

    /// Whether it has ended.
    pub(super) fn ended(&self) -> bool {
        // SAFETY: the handle came from CreateProcessWithLogonW and is open until drop.
        let waited = unsafe { WaitForSingleObject(self.0.hProcess, 0) };
        waited == WAIT_OBJECT_0
    }

    /// Its exit code once it ends, waiting up to `ms` milliseconds; `None` if still running.
    fn exit_code(&self, ms: u32) -> Option<u32> {
        // SAFETY: the handle came from CreateProcessWithLogonW and is open until drop.
        let waited = unsafe { WaitForSingleObject(self.0.hProcess, ms) };
        if waited != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: the handle is open; `code` is a live out-pointer.
        unsafe { GetExitCodeProcess(self.0.hProcess, &raw mut code) }.unwrap();
        Some(code)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // SAFETY: both handles came from CreateProcessWithLogonW and are closed once, here;
        // only this process is ended.
        unsafe {
            if !self.ended()
                && let Err(error) = TerminateProcess(self.0.hProcess, 1)
            {
                eprintln!("the test's process wasn't ended: {error}");
            }
            WaitForSingleObject(self.0.hProcess, 10_000);
            drop(CloseHandle(self.0.hThread));
            drop(CloseHandle(self.0.hProcess));
        }
    }
}

/// Runs `script` in `cmd` as `user`, whose profile is `profile`, and waits for it to end;
/// panics, with what it printed, unless it exits with 0. Its output goes to a file in the
/// profile, which the user may write and the profile delete takes away.
pub(super) fn run_in_cmd(user: &TestUser, profile: &Path, script: &str) {
    let (code, printed) = try_in_cmd(user, profile, script);
    assert_eq!(code, Some(0), "{script}\n{printed}");
    eprintln!("as the test user: {script}\n{printed}");
}

/// [`run_in_cmd`] without the check, its output going to a file in `folder`, which `user` must
/// be able to write: its exit code (`None` if it didn't end in time) and what it printed,
/// stdout and stderr together, read without failing on bytes that aren't UTF-8. Parentheses in
/// `script` must be quoted or escaped: it runs inside a `(...)` group.
pub(super) fn try_in_cmd(user: &TestUser, folder: &Path, script: &str) -> (Option<u32>, String) {
    let output = folder.join(OUTPUT);
    let cmd = format!(r"{}\cmd.exe", system32());
    // `/s`: cmd takes what is between the outer quotes as it is.
    let line = format!(
        r#""{cmd}" /d /s /c "({script}) > "{}" 2>&1""#,
        output.display()
    );
    let running = Running::start(user, &cmd, &line);
    let code = running.exit_code(COMMAND_MS);
    drop(running);
    let printed = match std::fs::read(&output) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => format!("(its output can't be read: {error})"),
    };
    (code, printed)
}
