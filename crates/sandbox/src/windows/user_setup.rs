//! [`UserSetup`]: the app's side of the elevated set-up (S15 `PR4h`), as R17 holds it through
//! [`SandboxSetup`]. It reads the set-up's state, starts the setup exe beside the app and waits
//! for it, and forgets a matter without admin.
//!
//! The exe's manifest asks for an administrator (`requireAdministrator`, D-B14), so starting it
//! with the verb `open` raises the one UAC prompt; there is no `runas` fallback (D-B4). It runs
//! hidden, with no error box from the shell (`SEE_MASK_FLAG_NO_UI`), and the wait has no limit:
//! R17 calls `run` off the actor thread. A prompt declined (1223) is [`SetupError::Refused`];
//! 1260 (a policy) and 1385 are failures with a named cause (`launch_error`). Once the exe ends,
//! its exit code and `setup-result.json` say how ([`result_file::outcome`]).
#![expect(
    unsafe_code,
    reason = "FFI to ShellExecuteExW and the started process's handle"
)]

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clauseo_protocol::MatterId;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::core::{PCWSTR, w};

use super::NtfsAcl;
use super::apply::forget_matter;
use super::ledger::{Ledger, ledger_path};
use super::marker::setup_state;
use super::matters::MatterSids;
use super::record::{sandbox_group_sid, sandbox_pool_sids};
use super::setup::args::quote;
use super::setup::around::Around;
use super::setup::launch_error;
use super::setup::result_file;
use super::sids::{Reserved, Sid};
use crate::{SandboxSetup, SetupError, SetupState};

/// `SW_HIDE`: the exe shows no window.
const SW_HIDE: i32 = 0;

/// The set-up for the Windows user running the app: `data_dir` is that user's data folder,
/// `exe` the setup exe (`clauseo-sandbox-setup.exe`, bundled beside `Clauseo.exe`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSetup {
    data_dir: PathBuf,
    exe: PathBuf,
}

impl UserSetup {
    /// The set-up for the data folder `data_dir`, run through the setup exe at `exe`.
    pub fn new(data_dir: PathBuf, exe: PathBuf) -> UserSetup {
        UserSetup { data_dir, exe }
    }

    /// Adds `n` pool users to a pool this user set up (D-B5): one prompt, as [`SandboxSetup::run`]
    /// raises. Every registered matter is fenced against the new users; no password is reset.
    pub fn extend(&self, n: u8) -> Result<(), SetupError> {
        self.set_up(n)
    }

    /// The elevated clean-up (D-B10, `PR4i`): one prompt.
    pub fn reclaim(&self) -> Result<(), SetupError> {
        let parent = std::process::id();
        self.run_reading(&format!("--reclaim --parent {parent}"))
    }

    /// `--set-up`, with `--add` when `add` is above 0.
    fn set_up(&self, add: u8) -> Result<(), SetupError> {
        let parent = std::process::id();
        let data_dir = self.data_dir.to_str().ok_or_else(|| {
            SetupError::Failed(format!(
                "the data folder {} isn't text",
                self.data_dir.display()
            ))
        })?;
        let params = format!("--set-up --parent {parent} --data-dir {}", quote(data_dir));
        if add > 0 {
            return self.run_reading(&format!("{params} --add {add}"));
        }
        self.run_reading(&params)
    }

    /// Starts the exe with `params` and reads its result file after: the last run's removed
    /// first, so a run that writes none is never read as another's.
    fn run_reading(&self, params: &str) -> Result<(), SetupError> {
        result_file::remove(&self.data_dir).map_err(|error| {
            SetupError::Failed(format!(
                "the last set-up's result can't be cleared: {error}"
            ))
        })?;
        let code = launch(&self.exe, params)?;
        result_file::outcome(code, Some(&result_file::read(&self.data_dir)))
    }
}

impl SandboxSetup for UserSetup {
    fn state(&self) -> SetupState {
        setup_state(&self.data_dir)
    }

    fn run(&self) -> Result<(), SetupError> {
        self.set_up(0)
    }

    /// `apply::forget_matter` through this user's ledger, then, once nothing of the matter's is
    /// left to remove, `MatterSids::release`, so its pool user can go to another matter.
    #[expect(
        clippy::disallowed_methods,
        reason = "the release's time in matters.json, the one clock a forget reads"
    )]
    fn forget_matter(&self, matter: &MatterId) -> Result<(), SetupError> {
        let failed = |why: String| SetupError::Failed(format!("forgetting {matter}: {why}"));
        let mut trusted: Vec<Sid> = sandbox_pool_sids().map_err(|e| failed(e.to_string()))?;
        if let Some(group) = sandbox_group_sid().map_err(|e| failed(e.to_string()))? {
            trusted.push(Sid::parse(&group).map_err(|e| failed(e.to_string()))?);
        }
        let reserved = Reserved::of_this_machine()?;
        let mut ledger = Ledger::open(ledger_path(&self.data_dir), &trusted, &reserved)
            .map_err(|e| failed(format!("the permission ledger: {e}")))?;
        let mut acl = NtfsAcl::new().map_err(|e| failed(e.to_string()))?;
        let mut around = Around::new(self.data_dir.clone());
        let forgotten = forget_matter(&mut ledger, *matter, &mut acl, &mut around)
            .map_err(|e| failed(e.to_string()))?;
        if forgotten.kept > 0 {
            return Err(failed(format!(
                "{} permission entries couldn't be removed yet; forget it again",
                forgotten.kept
            )));
        }
        MatterSids::load(&self.data_dir)?.release(matter, SystemTime::now())?;
        Ok(())
    }

    fn remove(&self) -> Result<(), SetupError> {
        let code = launch(&self.exe, "--remove")?;
        result_file::outcome(code, None)
    }
}

/// The started process's handle, closed on drop.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: the handle came from ShellExecuteExW (SEE_MASK_NOCLOSEPROCESS) and is closed
        // once, here. A failed close leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// Starts `exe` with `params` as the module says, waits for it, and gives its exit code. A
/// launch that fails is `launch_error` of its code.
fn launch(exe: &Path, params: &str) -> Result<u32, SetupError> {
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let parameters: Vec<u16> = params.encode_utf16().chain([0]).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: `info` is live and sized in `cbSize`; its strings are NUL-terminated and outlive
    // the call. The process handle it returns is owned by `Process` at once.
    let started = unsafe { ShellExecuteExW(&raw mut info) };
    if let Err(error) = started {
        let code = u32::try_from(error.code().0 & 0xFFFF).unwrap_or(u32::MAX);
        return Err(launch_error(code));
    }
    if info.hProcess.is_invalid() {
        return Err(SetupError::Failed(
            "Clauseo's set-up program started, but Windows gave no process to wait for".to_owned(),
        ));
    }
    let process = Process(info.hProcess);
    // SAFETY: the handle is live (owned by `process`) and has SYNCHRONIZE, as ShellExecuteExW
    // gives it.
    if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
        return Err(SetupError::Failed(format!(
            "waiting for Clauseo's set-up program failed ({})",
            std::io::Error::last_os_error()
        )));
    }
    let mut code = 0u32;
    // SAFETY: the handle is live and has PROCESS_QUERY_LIMITED_INFORMATION; `code` is a live
    // out-pointer.
    unsafe { GetExitCodeProcess(process.0, &raw mut code) }.map_err(|error| {
        SetupError::Failed(format!(
            "the set-up program's exit code can't be read ({error})"
        ))
    })?;
    Ok(code)
}

#[cfg(test)]
mod tests;
