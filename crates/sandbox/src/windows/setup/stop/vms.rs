//! The stop's WSL step (step 5b): a pool user's WSL 2 VM, or one WSL's containers made for it,
//! is a Hyper-V compute system that `wslservice.exe` runs as SYSTEM, so ending the user's
//! processes leaves it running. HCS (`computecore.dll`) names its systems by id, and WSL's
//! owner is the constant `"WSL"` for every user's VM, so only systems with that `Owner` are
//! looked at, and the user is read from the profile: the service makes
//! `%LOCALAPPDATA%\temp\<VM id>` (upper case, no braces) as the user for each VM it starts, and
//! deletes it when the VM ends (WSL `WslCoreVm.cpp:135,148`, at the commit 0049 cites).
//!
//! That folder can't prove a pool user's VM absent: WSL finds `%LOCALAPPDATA%` through the
//! user's own `User Shell Folders`, which the user may point elsewhere, and a folder it fails to
//! make (a pool user may deny itself the right) still starts the VM. So each WSL system is
//! identified positively. The lawyer's (`OwnerSid`'s) profile is checked first, at its default
//! `AppData\Local` and at its `Local AppData` redirect: a system whose folder is there is the
//! lawyer's and left alone, and a pool user whose profile also holds that folder planted it, and
//! is marked. Then a system whose folder is in a pool user's profile is that user's: it is
//! terminated, and the user marked for a reset. Any other WSL system (one whose folder can't be
//! found, or whose lawyer's folder can't be read) is unmapped: nothing terminates it, every pool
//! user is marked, and the stop ends at 48. A machine where another Windows account runs WSL too
//! ends its stops at 48 while that account's VM runs.
//!
//! The seam is [`VmStopper`], which `windows_hcs.rs` implements and the tests fake. The step
//! runs after the process passes, so no pool user's process is left to change its profile
//! while the folders are read, and each pool folder is read component by component without
//! following a link (`windows_hcs.rs`).

use std::time::Duration;

use super::{Failed, Passes, Pool, Stopper};
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

/// `Owner` of every compute system WSL makes.
pub const WSL_OWNER: &str = "WSL";

/// One entry of `HcsEnumerateComputeSystems`' result document: only its `Id` and `Owner` are
/// read.
#[derive(serde::Deserialize)]
struct Listed {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Owner")]
    owner: Option<String>,
}

/// The ids of the systems whose `Owner` is [`WSL_OWNER`] in `HcsEnumerateComputeSystems`'
/// result document, a JSON array of each system's properties; a document that isn't one is the
/// error (no path or id in it).
pub fn wsl_ids_from_json(document: &str) -> Result<Vec<String>, Failed> {
    let listed: Vec<Listed> = serde_json::from_str(document).map_err(|error| {
        // The parser's own words may quote the document; its line and column don't.
        format!(
            "HCS's list of compute systems isn't the JSON expected (line {}, column {})",
            error.line(),
            error.column()
        )
    })?;
    let wsl = listed
        .into_iter()
        .filter(|entry| entry.owner.as_deref() == Some(WSL_OWNER));
    Ok(wsl.map(|entry| entry.id).collect())
}

/// The folder a `Local AppData` value of `User Shell Folders` names, for the user whose profile
/// folder is `profile`: `%USERPROFILE%` (any case) at its start is `profile`, and a value with
/// no variable is taken as it is when it is absolute (a drive and `\`, or `\\`). Any other
/// variable, or a relative path, is `None`: it can't be read as the user would.
pub fn local_app_data(value: &str, profile: &str) -> Option<String> {
    const VARIABLE: &str = "%USERPROFILE%";
    let value = value.trim();
    let head = value.get(..VARIABLE.len());
    if head.is_some_and(|head| head.eq_ignore_ascii_case(VARIABLE)) {
        let rest = value.get(VARIABLE.len()..)?;
        return (!rest.contains('%')).then(|| format!("{profile}{rest}"));
    }
    let bytes = value.as_bytes();
    let drive = bytes.len() > 2
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1..3) == Some(b":\\".as_slice());
    let absolute = drive || value.starts_with("\\\\");
    (absolute && !value.contains('%')).then(|| value.to_owned())
}

/// What the WSL step reads and does, on top of [`Stopper`]'s.
pub trait VmStopper {
    /// The id of every HCS compute system whose `Owner` is [`WSL_OWNER`], from
    /// `HcsEnumerateComputeSystems`, waiting a bounded time: `None` when there is no HCS (no
    /// `computecore.dll`, or no `vmcompute` service), so no VM can run.
    fn compute_systems(&mut self) -> Result<Option<Vec<String>>, Failed>;
    /// Whether a WSL that runs VMs is installed: `WSLService` or the inbox `LxssManager`
    /// (which runs WSL 2 too, from Windows 10 2004 on). A read that fails counts as installed.
    fn wsl_installed(&mut self) -> bool;
    /// Whether `user`'s profile (`ProfileList`) holds the folder `AppData\Local\temp\<id>`:
    /// `false` with no profile, or nothing at any step of the way. A component that is a link
    /// (or a file where a folder belongs) on the way, or a read that fails, is the error.
    fn vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed>;
    /// Whether the lawyer `owner`'s profile holds `temp\<id>` under its `Local AppData`: at
    /// the default `AppData\Local`, or where `HKU\<owner>`'s `User Shell Folders` points it
    /// ([`local_app_data`]). `false` with no profile; a hive that isn't loaded has no
    /// redirect. A redirect that can't be read or expanded, or a read that fails, is the error.
    fn owner_vm_folder(&mut self, owner: &Sid, id: &VmId) -> Result<bool, Failed>;
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
    /// WSL systems neither the lawyer's nor a pool user's folder names, or whose lawyer's
    /// folder couldn't be read: each may be a pool user's, and none is terminated.
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

/// Step 5b, as the module says: each WSL system the lawyer's folder names is left, and a pool
/// user whose profile holds that folder too is added to `passes.suspects`; each a pool user's
/// folder names is terminated, its user added to `passes.found`; every other one is counted
/// unmapped and every pool user added to `passes.suspects`. A pool user whose folder can't be
/// read is added to `passes.suspects`. No HCS is nothing to do. A list that can't be read is the
/// error while a WSL that runs VMs is installed, and is logged and passed over otherwise (no
/// WSL, no WSL VM). A WSL system whose id isn't a GUID is unmapped.
pub(super) fn stop_vms(
    fs: &mut impl Stopper,
    pool: &Pool,
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
    for listed in &ids {
        let Some(id) = VmId::parse(listed) else {
            unmapped(pool, passes);
            continue;
        };
        match fs.owner_vm_folder(&pool.owner, &id) {
            Ok(true) => {
                // The lawyer's: a pool user holding its folder too planted it.
                for user in &pool.users {
                    if !matches!(fs.vm_folder(user, &id), Ok(false)) {
                        passes.suspects.insert(user.clone());
                    }
                }
                continue;
            }
            Ok(false) => {}
            Err(_why) => {
                unmapped(pool, passes);
                continue;
            }
        }
        let mut owner = None;
        for user in &pool.users {
            match fs.vm_folder(user, &id) {
                Ok(true) => {
                    owner = Some(user.clone());
                    break;
                }
                Ok(false) => {}
                Err(_why) => {
                    passes.suspects.insert(user.clone());
                }
            }
        }
        let Some(user) = owner else {
            unmapped(pool, passes);
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

/// A WSL system no folder maps: counted, and every pool user marked, as it may be any of theirs.
fn unmapped(pool: &Pool, passes: &mut Passes) {
    passes.vms.unmapped += 1;
    passes.suspects.extend(pool.users.iter().cloned());
}
