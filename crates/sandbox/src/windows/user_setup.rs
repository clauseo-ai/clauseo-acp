//! [`UserSetup`]: the app's side of the elevated set-up (S15 `PR4h`), as R17 holds it through
//! [`SandboxSetup`]. It reads the set-up's state, starts the setup exe's admin copy and waits
//! for it, and forgets a matter without admin.
//!
//! The one exe it starts is the admin copy in `%ProgramFiles%\Clauseo\Sandbox\`
//! (`admin_copy`), never the copy beside `Clauseo.exe`, which a per-user install keeps in a
//! folder the lawyer's own processes can write. Before every start the copy passes
//! `admin_copy::check` (there, its recorded SHA-256, a folder chain only administrators can
//! change), or nothing starts and the run is [`SetupError::Blocked`] with the cause, recorded
//! for `state` as a policy block is.
//!
//! The exe's manifest asks for an administrator (`requireAdministrator`, D-B14), so starting it
//! with the verb `open` raises the one UAC prompt; there is no `runas` fallback (D-B4). It runs
//! hidden, with no error box from the shell (`SEE_MASK_FLAG_NO_UI`), and the wait has no limit:
//! R17 calls `run` off the actor thread. A prompt declined (1223) is [`SetupError::Refused`];
//! a policy blocking the exe (1260) is [`SetupError::Blocked`], named for IT, and a 1385 from
//! the launch (the approving account's sign-in) a failure saying so (`launch_error`). Once the
//! exe ends, its exit code and `setup-result.json` say how ([`result_file::outcome`]): its
//! 1385 (a sandbox user's logon right) is [`SetupError::Blocked`] too, and a data folder not
//! owned by this user a failure saying so. `state` reports a block as [`SetupState::Blocked`]
//! until a set-up succeeds.
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
use super::admin_copy::os::check_here;
use super::apply::forget_matter;
use super::ledger::{Ledger, ledger_path};
use super::marker::setup_state;
use super::matters::MatterSids;
use super::reclaim::hand_back_after;
use super::record::{sandbox_group_sid, sandbox_pool_sids};
use super::setup::SetupExit;
use super::setup::args::quote;
use super::setup::around::Around;
use super::setup::launch_error;
use super::setup::result_file::{self, SetupResult};
use super::setup::taken_file;
use super::sids::{Reserved, Sid};
use crate::{SandboxSetup, SetupError, SetupState};

/// `SW_HIDE`: the exe shows no window.
const SW_HIDE: i32 = 0;

/// The set-up for the Windows user running the app: `data_dir` is that user's data folder.
/// The setup exe is always the admin copy, found and checked at each start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserSetup {
    data_dir: PathBuf,
}

impl UserSetup {
    /// The set-up for the data folder `data_dir`. No path to an exe is taken: the one started
    /// is the admin copy (`admin_copy::check_here`).
    pub fn new(data_dir: PathBuf) -> UserSetup {
        UserSetup { data_dir }
    }

    /// Adds `n` pool users to a pool this user set up (D-B5): one prompt, as [`SandboxSetup::run`]
    /// raises. Every registered matter is fenced against the new users; no password is reset.
    pub fn extend(&self, n: u8) -> Result<(), SetupError> {
        self.set_up(n)
    }

    /// The elevated clean-up (D-B10, S15 `PR4i`): one prompt. `--reclaim` makes this user the
    /// owner of what a sandbox user owns and this user can't hand back, in the trees the ledger
    /// owes a clean-up (`reclaim::targets`), taking their `OWNER RIGHTS` entries off first, and
    /// changes nothing else; then, unelevated, the
    /// same trees are handed back (`reclaim::hand_back_after`), which cleans what was taken of
    /// its old owner's entries, and the items the exe reports taken of the denies an agent left
    /// of this user. The app then retries each owed `forget_matter` first, then
    /// the launch that refused.
    ///
    /// The hand-back runs when the exe is done, and also when it failed partway having taken
    /// something (`taken` in its result file), so nothing it took is left uncleaned; the exe's
    /// failure is still what this returns then.
    pub fn reclaim(&self) -> Result<(), SetupError> {
        taken_file::remove(&self.data_dir).map_err(|error| {
            SetupError::Failed(format!(
                "the last clean-up's list can't be cleared: {error}"
            ))
        })?;
        let parent = std::process::id();
        let (code, read) = self.run_raw(&format!("--reclaim --parent {parent}"))?;
        let taken = read
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .and_then(|r| r.taken);
        let partial = code == u32::from(SetupExit::Failed.as_code()) && taken.unwrap_or(0) > 0;
        let handed = (code == 0 || partial).then(|| self.hand_back_reclaimed());
        result_file::outcome(code, Some(&read))?;
        handed.unwrap_or(Ok(()))
    }

    /// The unelevated half of [`UserSetup::reclaim`]: the ledger read again, as this user, the
    /// items the exe reported taken (`reclaim-taken.json`) recorded in it, and each target
    /// handed back with the record's pool users, the taken items' denies of this user stripped
    /// (`reclaim::hand_back_after`). A list that can't be read is logged, and the hand-back runs
    /// with what the ledger already lists.
    fn hand_back_reclaimed(&self) -> Result<(), SetupError> {
        let failed =
            |why: String| SetupError::Failed(format!("handing back what was taken: {why}"));
        let (pool, trusted) = pool_and_trusted().map_err(failed)?;
        let reserved = Reserved::of_this_machine()?;
        let mut ledger = Ledger::open(ledger_path(&self.data_dir), &trusted, &reserved)
            .map_err(|e| failed(format!("the permission ledger: {e}")))?;
        let taken = taken_file::read(&self.data_dir).unwrap_or_else(|_why| {
            // Its words may name a path: kept out of the log.
            tracing::warn!("reclaim-taken.json can't be read; only the ledger's items lose denies");
            Vec::new()
        });
        let matters = MatterSids::load(&self.data_dir)?;
        let mut acl = NtfsAcl::new().map_err(|e| failed(e.to_string()))?;
        let handed = hand_back_after(
            &mut acl,
            &mut ledger,
            matters.rows(),
            &taken,
            &pool,
            &trusted,
        )
        .map_err(|e| failed(format!("the permission ledger: {e}")))?;
        if let Err(error) = taken_file::remove(&self.data_dir) {
            tracing::warn!(error = %error.kind(), "the clean-up's list of items taken stays");
        }
        if handed.refused.is_empty() {
            return Ok(());
        }
        Err(failed(format!(
            "{} items still can't be handed back; launch or forget the matter to see which",
            handed.refused.len()
        )))
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
        let (code, read) = self.run_raw(params)?;
        result_file::outcome(code, Some(&read))
    }

    /// [`UserSetup::run_reading`]'s run: the exit code and the result file as read after.
    fn run_raw(
        &self,
        params: &str,
    ) -> Result<(u32, Result<Option<SetupResult>, String>), SetupError> {
        result_file::remove(&self.data_dir).map_err(|error| {
            SetupError::Failed(format!(
                "the last set-up's result can't be cleared: {error}"
            ))
        })?;
        let code = match launch(params) {
            Ok(code) => code,
            Err(SetupError::Blocked(blocked)) => {
                // Nothing ran to say so: the app records it, so `state` reports it until a
                // later set-up succeeds.
                let result = SetupResult {
                    blocked: Some(blocked),
                    ..SetupResult::failed(
                        SetupExit::Failed,
                        "starting the set-up program",
                        blocked.to_string(),
                    )
                };
                if let Err(error) = result_file::write(&self.data_dir, &result) {
                    tracing::warn!(%error, "the blocked set-up couldn't be recorded");
                }
                return Err(SetupError::Blocked(blocked));
            }
            Err(error) => return Err(error),
        };
        Ok((code, result_file::read(&self.data_dir)))
    }
}

impl SandboxSetup for UserSetup {
    /// The marker and record's state, or [`SetupState::Blocked`] while the last set-up was
    /// blocked and nothing is set up since (`result_file::state_with`). Never writes: the result
    /// file is read unchanged, and ignored unless it is this user's alone
    /// (`result_file::read_unchanged`); `run` repairs it.
    fn state(&self) -> SetupState {
        let last = result_file::read_unchanged(&self.data_dir);
        result_file::state_with(setup_state(&self.data_dir), &last)
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
        let (_, trusted) = pool_and_trusted().map_err(failed)?;
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
        let code = launch("--remove")?;
        result_file::outcome(code, None)
    }
}

/// The record's pool users, and the SIDs a ledger trusts: those and their group.
fn pool_and_trusted() -> Result<(Vec<Sid>, Vec<Sid>), String> {
    let pool: Vec<Sid> = sandbox_pool_sids().map_err(|e| e.to_string())?;
    let mut trusted = pool.clone();
    if let Some(group) = sandbox_group_sid().map_err(|e| e.to_string())? {
        trusted.push(Sid::parse(&group).map_err(|e| e.to_string())?);
    }
    Ok((pool, trusted))
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

/// Starts the admin copy with `params` as the module says, once it passes
/// `admin_copy::check_here` (else [`SetupError::Blocked`], nothing started), waits for it, and
/// gives its exit code. A launch that fails is `launch_error` of its code.
fn launch(params: &str) -> Result<u32, SetupError> {
    let exe = check_here().map_err(SetupError::Blocked)?;
    start(&exe, params)
}

/// Starts `exe`, which [`launch`] checked, with `params`, waits for it and gives its exit code.
fn start(exe: &Path, params: &str) -> Result<u32, SetupError> {
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
