//! The start's first question: does this process run inside the agent sandbox? A jailed agent can
//! start this executable (the pre-write guard hook needs it to: on Windows the sandbox grants
//! read and execute on `Clauseo.exe`, and the macOS profile allows `process-exec`), and the
//! whole app must never start there.
//!
//! Contract:
//! - `main` calls [`launch`] with the arguments after the program and nothing else has run.
//!   [`launch`] answers `--pre-write-guard` first ([`guard_requested`]): the guard runs inside
//!   the sandbox by design, so it never asks. `--delete-app-data` (the uninstaller's,
//!   `delete_app_data.rs`) asks next and runs only on a plain `Ok(false)`: sandboxed, it refuses
//!   as below; a failed probe refuses it with `delete_app_data::FAILED` and deletes nothing.
//!   Every other start asks the [`SandboxProbe`] next, before logging, settings, the Tauri
//!   builder and its single-instance plugin: a jailed start must not forward its arguments (deep
//!   links included) to a running window.
//! - Sandboxed: one line, [`REFUSAL`], on stderr (no path, no secret) and exit code
//!   [`SANDBOXED_EXIT`]; no window, nothing else runs. Not sandboxed: the app starts.
//! - The probe failing is not "sandboxed": one stderr line says the check failed and why, and
//!   the app starts, so a broken API never locks a lawyer out. What counts as a positive answer
//!   rather than a failure is each OS's rule below.
//! - [`SystemProbe`] on Windows (`signals.rs` decides, `windows.rs` reads): the process token
//!   is an `AppContainer` (`TokenIsAppContainer`), has restricting SIDs
//!   (`GetTokenInformation(TokenRestrictedSids)`, a non-empty list), has an integrity level
//!   below Medium (`TokenIntegrityLevel`'s last sub-authority under
//!   `SECURITY_MANDATORY_MEDIUM_RID`, 0x2000; pool users always run at Low, so this holds for a
//!   stray pool process whatever the record says, and a level that can't be read for any reason
//!   counts as sandboxed), or its user (`TokenUser`) is any of S15's pool users, compared with
//!   `EqualSid` against each SID S15's set-up records in `HKLM\SOFTWARE\Clauseo\Sandbox`, value
//!   `PoolSids` (a `REG_MULTI_SZ` of SIDs in the `S-1-…` form, 64-bit view; missing means no
//!   pool). `ERROR_ACCESS_DENIED` on the
//!   token or the record, a record that is not a `REG_MULTI_SZ`, or one that is not a list of
//!   `S-1-…` SIDs (an empty string inside, an alias such as `BA`, a repeat), also counts as
//!   sandboxed; any other failure fails open as above. On macOS
//!   `sandbox_check(getpid(), NULL, SANDBOX_FILTER_NONE)` is non-zero (its -1 error counts as
//!   sandboxed). Linux has no check: the Linux agent jail is not a target.
//! - Known revisit: a sandboxed packaging of Clauseo itself (the Mac App Store, or MSIX with an
//!   `AppContainer`) would trip this check (`docs/decisions/0012-desktop-bridge.md`).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(windows, test))]
mod signals;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::io::{self, Write};

use crate::{delete_app_data, guard_requested};

/// The exit code of a start refused inside the agent sandbox: sysexits' `EX_NOPERM`, apart from
/// the guard's own codes (0, and `clauseo_host::pre_write_guard::DENY`).
pub const SANDBOXED_EXIT: i32 = 77;

/// The one stderr line of a refused start.
pub const REFUSAL: &str = "Clauseo will not start inside the agent sandbox.";

/// Whether this process runs inside the agent sandbox. A seam, so tests can give each answer.
pub trait SandboxProbe {
    /// `Ok(true)` inside the sandbox, `Ok(false)` outside, `Err` when the OS could not say.
    fn sandboxed(&self) -> io::Result<bool>;
}

/// The OS's answer (the contract above has each OS's call).
pub struct SystemProbe;

impl SandboxProbe for SystemProbe {
    fn sandboxed(&self) -> io::Result<bool> {
        #[cfg(windows)]
        return windows::sandboxed();
        #[cfg(target_os = "macos")]
        return Ok(macos::sandboxed());
        // The Linux agent jail is not a target: nothing to ask.
        #[cfg(not(any(windows, target_os = "macos")))]
        return Ok(false);
    }
}

/// What a start does after the check: the guard, or the app. A seam, so tests can count them.
pub trait Stages {
    /// Runs the pre-write guard with the arguments after its flag; returns its exit code.
    fn guard(&mut self, args: &[OsString]) -> i32;
    /// Runs the uninstaller's deletion with the arguments after its flag; returns its exit code.
    fn delete_app_data(&mut self, args: &[OsString]) -> i32;
    /// Builds and runs the app until it exits.
    fn app(&mut self) -> tauri::Result<()>;
}

/// The real stages: `clauseo_host::pre_write_guard::run` on stdin, stdout and stderr,
/// `delete_app_data::run` on stderr, and [`crate::run`].
pub struct AppStages;

impl Stages for AppStages {
    fn guard(&mut self, args: &[OsString]) -> i32 {
        clauseo_host::pre_write_guard::run(args, io::stdin().lock(), io::stdout(), io::stderr())
    }

    fn delete_app_data(&mut self, args: &[OsString]) -> i32 {
        delete_app_data::run(args, &mut io::stderr())
    }

    fn app(&mut self) -> tauri::Result<()> {
        crate::run()
    }
}

/// How a start ended: `main` exits with the code, or returns the app's result.
#[derive(Debug)]
pub enum Launched {
    /// Exit with this code: the guard's, the deletion's, or [`SANDBOXED_EXIT`].
    Exit(i32),
    /// The app ran; its result.
    App(tauri::Result<()>),
}

/// The start: the guard when `args` (the arguments after the program) ask for it, else the
/// sandbox check, then the uninstaller's deletion when they ask for it and the probe said no, or
/// the app unless the probe said sandboxed. Lines go to `stderr`.
pub fn launch(
    args: &[OsString],
    probe: &dyn SandboxProbe,
    stderr: &mut dyn Write,
    stages: &mut dyn Stages,
) -> Launched {
    if let Some((first, rest)) = args.split_first()
        && guard_requested(Some(first.as_os_str()))
    {
        return Launched::Exit(stages.guard(rest));
    }
    if let Some((first, rest)) = args.split_first()
        && delete_app_data::requested(first)
    {
        return Launched::Exit(match probe.sandboxed() {
            Ok(false) => stages.delete_app_data(rest),
            Ok(true) => {
                say(stderr, REFUSAL);
                SANDBOXED_EXIT
            }
            Err(error) => {
                say(
                    stderr,
                    &format!(
                        "Clauseo could not tell whether it runs in the agent sandbox ({error}); \
                         nothing deleted."
                    ),
                );
                delete_app_data::FAILED
            }
        });
    }
    match probe.sandboxed() {
        Ok(true) => {
            say(stderr, REFUSAL);
            return Launched::Exit(SANDBOXED_EXIT);
        }
        Ok(false) => {}
        Err(error) => say(
            stderr,
            &format!(
                "Clauseo could not tell whether it runs in the agent sandbox ({error}); starting."
            ),
        ),
    }
    Launched::App(stages.app())
}

/// Writes `line` and a newline to `stderr`. Nothing more can be done when stderr is closed; the
/// exit code still says what happened.
fn say(stderr: &mut dyn Write, line: &str) {
    drop(writeln!(stderr, "{line}").and_then(|()| stderr.flush()));
}
