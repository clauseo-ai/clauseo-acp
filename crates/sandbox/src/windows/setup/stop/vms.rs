//! The stop's WSL step (step 5b): a pool user's WSL 2 VM, or one WSL's containers made for it,
//! is a Hyper-V compute system that `wslservice.exe` runs as SYSTEM, so ending the user's
//! processes leaves it running. HCS (`computecore.dll`) names its systems by id only, and WSL's
//! owner is the constant `"WSL"` for every user's VM, so the user is read from the profile: the
//! service makes `%LOCALAPPDATA%\temp\<VM id>` (upper case, no braces) as the user for each VM
//! it starts, and deletes it when the VM ends (WSL `WslCoreVm.cpp:135,148`, at the commit 0049
//! cites). A system whose folder is in a pool user's profile is that user's: it is terminated,
//! and the user marked for a reset. Every other system is left alone: the lawyer's VMs, other
//! users', and containers that aren't WSL's.
//!
//! The seam is [`VmStopper`], which `windows_hcs.rs` implements and the tests fake. The step
//! runs after the process passes, so no pool user's process is left to change its profile
//! while the folders are read, and each folder is read component by component without following
//! a link (`windows_hcs.rs`).

use std::time::Duration;

use super::{Failed, Passes, Stopper};
use crate::windows::sids::Sid;

/// How long HCS's list of compute systems is waited for, at most.
pub const LIST_WAIT: Duration = Duration::from_secs(3);
/// How long one VM's termination is waited for, at most.
pub const VM_WAIT: Duration = Duration::from_secs(5);
/// How long the step's terminations may take all together: the task's 2 minutes hold the
/// passes' 75 s, [`LIST_WAIT`], this, and the lock's 30 s wait.
pub const VMS_BUDGET: Duration = Duration::from_secs(10);

/// A compute system's id that is a GUID, held upper case with no braces: the form WSL names
/// the VM's folder with, and the only form this step ever puts in a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmId(String);

impl VmId {
    /// `text` as a GUID (`8-4-4-4-12` hex digits, braces allowed, any case); `None` otherwise.
    pub fn parse(text: &str) -> Option<VmId> {
        let bare = text
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
            .unwrap_or(text);
        let groups: Vec<&str> = bare.split('-').collect();
        let sizes = groups.iter().map(|group| group.len());
        let shaped = groups.len() == 5 && sizes.eq([8, 4, 4, 4, 12]);
        let hex = bare.chars().all(|c| c == '-' || c.is_ascii_hexdigit());
        (shaped && hex).then(|| VmId(bare.to_ascii_uppercase()))
    }

    /// The id, upper case with no braces.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One entry of `HcsEnumerateComputeSystems`' result document: only its `Id` is read.
#[derive(serde::Deserialize)]
struct Listed {
    #[serde(rename = "Id")]
    id: String,
}

/// The ids in `HcsEnumerateComputeSystems`' result document, a JSON array of each system's
/// properties; a document that isn't one is the error (no path or id in it).
pub fn ids_from_json(document: &str) -> Result<Vec<String>, Failed> {
    let listed: Vec<Listed> = serde_json::from_str(document).map_err(|error| {
        // The parser's own words may quote the document; its line and column don't.
        format!(
            "HCS's list of compute systems isn't the JSON expected (line {}, column {})",
            error.line(),
            error.column()
        )
    })?;
    Ok(listed.into_iter().map(|entry| entry.id).collect())
}

/// What the WSL step reads and does, on top of [`Stopper`]'s.
pub trait VmStopper {
    /// Every HCS compute system's id, from `HcsEnumerateComputeSystems`, waiting a bounded
    /// time: `None` when there is no HCS (no `computecore.dll`, or no `vmcompute` service), so
    /// no VM can run.
    fn compute_systems(&mut self) -> Result<Option<Vec<String>>, Failed>;
    /// Whether a WSL that runs VMs is installed: `WSLService` or the inbox `LxssManager`
    /// (which runs WSL 2 too, from Windows 10 2004 on). A read that fails counts as installed.
    fn wsl_installed(&mut self) -> bool;
    /// Whether `user`'s profile (`ProfileList`) holds the folder `AppData\Local\temp\<id>`:
    /// `false` with no profile, or nothing at any step of the way. A component that is a link
    /// (or a file where a folder belongs) on the way, or a read that fails, is the error.
    fn vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed>;
    /// Terminates the compute system `id` and waits up to `wait` for it to end; one already
    /// gone is `Ok`.
    fn terminate_vm(&mut self, id: &VmId, wait: Duration) -> Result<(), Failed>;
}

/// What the step did, for the log line and the exit.
#[derive(Debug, Default)]
pub(super) struct Vms {
    /// HCS was there and listed its systems.
    pub(super) listed: bool,
    /// Pool users' VMs terminated.
    pub(super) ended: usize,
    /// Pool users' VMs not terminated: a termination that failed or ran out of time, or none
    /// tried once [`VMS_BUDGET`] was spent.
    pub(super) left: usize,
    /// VMs no pool user's folder names, where a pool user's folder couldn't be read: each may
    /// be that user's.
    pub(super) unmapped: usize,
}

impl Vms {
    /// The log line's part, counts only; empty when HCS wasn't there.
    pub(super) fn counts(&self) -> String {
        if !self.listed {
            return String::new();
        }
        format!(
            ", {} pool WSL VMs ended, {} not, {} unmapped",
            self.ended, self.left, self.unmapped
        )
    }
}

/// Step 5b: each compute system whose folder a pool user's profile holds is terminated, its
/// user added to `passes.found`; a pool user whose folder can't be read is added to
/// `passes.suspects`. No HCS is nothing to do. A list that can't be read is the error while a
/// WSL that runs VMs is installed, and is logged and passed over otherwise (no WSL, no WSL VM).
/// An id that isn't a GUID isn't WSL's, and is left.
pub(super) fn stop_vms(
    fs: &mut impl Stopper,
    pool: &[Sid],
    passes: &mut Passes,
) -> Result<(), String> {
    let ids = match fs.compute_systems() {
        Ok(None) => return Ok(()),
        Ok(Some(ids)) => ids,
        Err(why) if fs.wsl_installed() => {
            return Err(format!("HCS's compute systems couldn't be listed: {why}"));
        }
        Err(why) => {
            tracing::warn!(%why, "HCS's compute systems couldn't be listed, with no WSL here");
            return Ok(());
        }
    };
    passes.vms.listed = true;
    let began = fs.elapsed();
    for id in ids.iter().filter_map(|id| VmId::parse(id)) {
        let mut owner = None;
        let mut unread = false;
        for user in pool {
            match fs.vm_folder(user, &id) {
                Ok(true) => {
                    owner = Some(user.clone());
                    break;
                }
                Ok(false) => {}
                Err(_why) => {
                    unread = true;
                    passes.suspects.insert(user.clone());
                }
            }
        }
        let Some(user) = owner else {
            passes.vms.unmapped += usize::from(unread);
            continue;
        };
        passes.found.insert(user);
        let spent = fs.elapsed().saturating_sub(began);
        let wait = VM_WAIT.min(VMS_BUDGET.saturating_sub(spent));
        if wait.is_zero() {
            passes.vms.left += 1;
            continue;
        }
        match fs.terminate_vm(&id, wait) {
            Ok(()) => passes.vms.ended += 1,
            Err(_why) => passes.vms.left += 1,
        }
    }
    Ok(())
}
