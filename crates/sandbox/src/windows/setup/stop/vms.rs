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
//! make (a pool user may deny itself the right) still starts the VM. So the profiles no pool
//! user can write are checked first: the lawyer's (`OwnerSid`'s), then every other profile
//! `ProfileList` lists that isn't a pool user's, each at its default `AppData\Local` and at the
//! `Local AppData` redirect its hive names (loaded, or loaded read-only for the read). A system
//! whose folder is there is that account's and left alone, and a pool user whose profile also
//! holds that folder planted it, and is marked. Then a system whose folder is in a pool user's
//! profile is that user's: it is terminated, and the user marked for a reset. A system whose
//! folder is in no profile is most likely a pool user's that hid its folder: every pool user is
//! marked, and it is terminated too, unless a non-pool profile (or the list of them) couldn't be
//! read anywhere in the step, as it could be that account's. Another account's profile that
//! can't be read is otherwise passed over; the lawyer's can't be: a system whose lawyer's folder
//! can't be read is unmapped even with a pool user's folder (that pool user may have planted
//! it), as is one left running for want of a read, or whose id isn't a GUID: nothing terminates
//! it, every pool user is marked, and the stop ends at 48.
//!
//! The whole step is gated on evidence that a pool user started WSL ([`evidence`]), recorded
//! before any process is stopped: a pool user's profile holds `AppData\Local\temp\<GUID>`, its
//! loaded hive holds a WSL distribution registration (`Software\...\Lxss\*`), or a pass's
//! snapshot saw one of its processes that is WSL's (`wsl.exe`, `wslhost.exe`, `wslrelay.exe`,
//! `wslg.exe`, or a WSL 1 pico process). A read of the first two that fails counts as evidence.
//! With none, the step asks HCS nothing (`computecore.dll` isn't even loaded) and ends nothing:
//! the deny should keep the pool out of WSL, so on a real machine the step runs only when a pool
//! user demonstrably got in, and a mapping mistake can't end the lawyer's VMs in the normal case.
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
/// How long the step's terminations may take all together. The task's 2 minutes hold the
/// passes' 75 s, [`LIST_WAIT`] and this, but not every wait for the set-up lock: each taking
/// (the reservation, each mark, the completion) may wait up to 30 s, so a run that meets a
/// slow holder can pass the limit and be ended. So each user is marked before its VM is
/// terminated, and a run ended mid-step has marked every user it found.
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

/// `Owner` of every compute system WSL makes. Verified for the Store WSL's VMs only; whether
/// WSL's containers (WSLC) and the inbox WSL name the same owner is unconfirmed (a VM row).
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
    /// Whether a non-pool user's (the lawyer's, or another account's) profile holds `temp\<id>`
    /// under its `Local AppData`: at the default `AppData\Local`, or where its hive's `User
    /// Shell Folders` points it ([`local_app_data`]): `HKU\<user>` when the hive is loaded,
    /// else its `NTUSER.DAT` loaded read-only for the read. `false` with no profile, or with no
    /// `NTUSER.DAT`. A hive or redirect that can't be read or expanded, or a read that fails,
    /// is the error.
    fn non_pool_vm_folder(&mut self, user: &Sid, id: &VmId) -> Result<bool, Failed>;
    /// Every SID `ProfileList` lists (a subkey whose name isn't a SID is passed over).
    fn profile_sids(&mut self) -> Result<Vec<Sid>, Failed>;
    /// Whether the pool user `user`'s profile holds a folder (or anything) named as a GUID in
    /// `AppData\Local\temp`, read from the profile folder down without following a link:
    /// `false` with no profile or no such folder. A link on the way, or a read that fails, is
    /// the error.
    fn temp_vm_folder(&mut self, user: &Sid) -> Result<bool, Failed>;
    /// Whether the pool user `user`'s loaded hive holds a WSL distribution registration (a
    /// subkey of `HKU\<user>\Software\Microsoft\Windows\CurrentVersion\Lxss`): `false` when
    /// the hive isn't loaded or the key is missing or empty. A read that fails is the error.
    fn lxss_registered(&mut self, user: &Sid) -> Result<bool, Failed>;
    /// Terminates the compute system `id` and waits up to `wait` for it to end; one already
    /// gone is `Ok`.
    fn terminate_vm(&mut self, id: &VmId, wait: Duration) -> Result<(), Failed>;
}

/// What the step did, for the log line and the exit.
#[derive(Debug, Default)]
pub(super) struct Vms {
    /// HCS was there and listed its systems.
    pub(super) listed: bool,
    /// Pool users' VMs terminated, those in no profile included.
    pub(super) ended: usize,
    /// Pool users' VMs not terminated: a termination that failed or ran out of time, or none
    /// tried once [`VMS_BUDGET`] was spent.
    pub(super) left: usize,
    /// WSL systems left running though they may be a pool user's: the lawyer's folder couldn't
    /// be read, the id isn't a GUID, or the folder is in no profile while a non-pool profile
    /// couldn't be read.
    pub(super) unmapped: usize,
    /// WSL systems whose folder is in no profile, taken as a pool user's (each counted in
    /// `ended` or `left` too).
    pub(super) nowhere: usize,
    /// The step was skipped: no evidence that a pool user started WSL.
    pub(super) skipped: bool,
    /// Evidence that a pool user started WSL, recorded before any process is stopped
    /// ([`evidence`], and each pass's snapshot): without it the step is skipped.
    pub(super) evidence: bool,
}

impl Vms {
    /// The log line's part, counts only; empty when HCS wasn't there.
    pub(super) fn counts(&self) -> String {
        if self.skipped {
            return ", WSL VM step skipped: no pool WSL evidence".to_owned();
        }
        if !self.listed {
            return String::new();
        }
        format!(
            ", {} pool WSL VMs ended, {} not, {} unmapped, {} in no profile",
            self.ended, self.left, self.unmapped, self.nowhere
        )
    }
}

/// Step 5b, as the module says (users marked under the set-up lock before each termination),
/// once there is evidence (`passes.vms.evidence`); with none, nothing is asked or ended:
/// each WSL system a non-pool profile's folder names (the lawyer's first) is left, and a pool
/// user whose profile holds that folder too is added to `passes.suspects`; each a pool user's
/// folder names is terminated, its user added to `passes.found`; each in no profile adds every
/// pool user to `passes.suspects` and is terminated, unless a non-pool read failed anywhere in
/// the step, when it is unmapped. One whose lawyer's folder can't be read, or whose id isn't a
/// GUID, is unmapped and adds every pool user to `passes.suspects`; another non-pool profile
/// that can't be read is passed over. A pool
/// user whose folder can't be read is added to `passes.suspects`. No HCS is nothing to do. A
/// list that can't be read is the error while a WSL that runs VMs is installed, and is logged
/// and passed over otherwise (no WSL, no WSL VM).
pub(super) fn stop_vms(
    fs: &mut impl Stopper,
    pool: &Pool,
    passes: &mut Passes,
) -> Result<(), String> {
    if !passes.vms.evidence {
        passes.vms.skipped = true;
        return Ok(());
    }
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
    // The profiles no pool user can write: the lawyer's first, then every other one listed.
    let (others, mut unread) = match fs.profile_sids() {
        Ok(sids) => {
            let other = |sid: &Sid| *sid != pool.owner && !pool.users.contains(sid);
            (sids.into_iter().filter(other).collect(), false)
        }
        Err(_why) => (Vec::new(), true),
    };
    let mut nowhere = Vec::new();
    for listed in &ids {
        let Some(id) = VmId::parse(listed) else {
            unmapped(pool, passes);
            continue;
        };
        match non_pool_folder(fs, &pool.owner, &others, &id, &mut unread) {
            Held::Yes => {
                // A non-pool account's: a pool user holding its folder too planted it.
                for user in &pool.users {
                    if !matches!(fs.vm_folder(user, &id), Ok(false)) {
                        passes.suspects.insert(user.clone());
                    }
                }
                continue;
            }
            Held::Unknown => {
                unread = true;
                unmapped(pool, passes);
                continue;
            }
            Held::No => {}
        }
        match pool_user(fs, pool, &id, passes) {
            Some(user) => {
                passes.found.insert(user);
                end(fs, pool, &id, began, passes);
            }
            None => nowhere.push(id),
        }
    }
    // In no profile: most likely a pool user's VM that hid its folder, so it is ended, every
    // pool user marked first; but not while a non-pool profile couldn't be read, as it may be
    // that account's.
    for id in nowhere {
        if unread {
            unmapped(pool, passes);
            continue;
        }
        passes.suspects.extend(pool.users.iter().cloned());
        passes.vms.nowhere += 1;
        end(fs, pool, &id, began, passes);
    }
    Ok(())
}

/// Before the passes, the evidence that outlives a pool user's processes: a pool user's profile
/// holding `AppData\Local\temp\<GUID>`, or its loaded hive a WSL distribution registration. A
/// read that fails counts as evidence (a pool user may make its own profile unreadable). Each
/// pass adds the third kind, a pool user's WSL process, before it stops anything.
pub(super) fn evidence(fs: &mut impl Stopper, pool: &[Sid], passes: &mut Passes) {
    for user in pool {
        if !matches!(fs.temp_vm_folder(user), Ok(false))
            || !matches!(fs.lxss_registered(user), Ok(false))
        {
            passes.vms.evidence = true;
            return;
        }
    }
}

/// Whether a non-pool profile holds a VM's folder.
enum Held {
    /// One of them does.
    Yes,
    /// None does as far as was read, but the lawyer's couldn't be read.
    Unknown,
    /// None of those read does.
    No,
}

/// Whether the lawyer's profile or one of `others` holds `id`'s folder, the lawyer's read
/// first. Another's that can't be read is passed over, and sets `unread`.
fn non_pool_folder(
    fs: &mut impl Stopper,
    owner: &Sid,
    others: &[Sid],
    id: &VmId,
    unread: &mut bool,
) -> Held {
    let mut held = Held::No;
    match fs.non_pool_vm_folder(owner, id) {
        Ok(true) => return Held::Yes,
        Ok(false) => {}
        Err(_why) => held = Held::Unknown,
    }
    for user in others {
        match fs.non_pool_vm_folder(user, id) {
            Ok(true) => return Held::Yes,
            Ok(false) => {}
            Err(_why) => *unread = true,
        }
    }
    held
}

/// The pool user whose profile holds `id`'s folder; one whose folder can't be read is added
/// to `passes.suspects`.
fn pool_user(fs: &mut impl Stopper, pool: &Pool, id: &VmId, passes: &mut Passes) -> Option<Sid> {
    for user in &pool.users {
        match fs.vm_folder(user, id) {
            Ok(true) => return Some(user.clone()),
            Ok(false) => {}
            Err(_why) => {
                passes.suspects.insert(user.clone());
            }
        }
    }
    None
}

/// Terminates `id` within what is left of [`VMS_BUDGET`] since `began`, the users found so
/// far marked first; one not ended (a failure, a wait that ran out, or none tried) is counted
/// left.
fn end(fs: &mut impl Stopper, pool: &Pool, id: &VmId, began: Duration, passes: &mut Passes) {
    let spent = fs.elapsed().saturating_sub(began);
    let wait = VM_WAIT.min(VMS_BUDGET.saturating_sub(spent));
    if wait.is_zero() {
        passes.vms.left += 1;
        return;
    }
    // Marked first: a run the task's limit ends during the wait has marked the user.
    super::mark_new(fs, &pool.users, passes);
    match fs.terminate_vm(id, wait) {
        Ok(()) => passes.vms.ended += 1,
        Err(_why) => passes.vms.left += 1,
    }
}

/// A WSL system left running that may be a pool user's: counted, and every pool user marked.
fn unmapped(pool: &Pool, passes: &mut Passes) {
    passes.vms.unmapped += 1;
    passes.suspects.extend(pool.users.iter().cloned());
}
