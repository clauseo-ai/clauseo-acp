//! [`WindowsRemoval`]: the real [`Removal`], over the calls set-up's `WindowsMachine` uses
//! (S15 `PR4e` to `PR4h`): the record, the lock (waited for, here), the accounts, the profiles,
//! the stray processes. The owner is optional: a per-machine removal may have no parent to
//! trust (a launch from session 0) or one that isn't the owner (LOCAL SYSTEM under Intune), and
//! then nothing runs as the owner.
//!
//! The record's `DataDir` is a path the owner can influence. Before any use it is held to the
//! plain drive path `--data-dir` is held to (`args::data_dir_of`), then resolved one component
//! at a time with no junction or link (`private_dir::resolve_folder`), and only its final path
//! is used. Either way the folders are held to `remove::owners`: the folder to set-up's rule
//! (the owner, or Administrators for an owner in Administrators, read by
//! `admin_member::in_administrators`), its `sandbox` folder to the rest of the sandbox's (the
//! owner, Administrators or SYSTEM), since an elevated process makes Administrators the owner
//! of what it creates (the built-in Administrator, or UAC off). For an administrator acting for
//! an absent owner, the ledger is only read, and a matter folder is held to the `sandbox`
//! folder's rule (`admin_target`): all the removal does there is take our own pool and group
//! entries off, and our Low label (`ledger_work` says when). Causes name the kind of a failure,
//! never the path.
//!
//! The owner is acted as only with a standard user's token (`remove::acting_token`): the
//! parent's own, or a copy lowered to Administrators deny-only and Medium integrity when the
//! parent is elevated, as a per-machine uninstaller is. If lowering fails, nothing runs as the
//! owner and the removal takes the administrator's path. The ledger is read through a handle
//! opened relative to the checked `sandbox` folder's handle, never through its path.
//!
//! Every call runs on the thread that made the machine (the mutex and the impersonation are the
//! thread's), so it is not `Send`.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use clauseo_protocol::MatterId;

use super::super::accounts::{self, Kind};
use super::super::answers::DENY_REMOTE_INTERACTIVE_RIGHT;
use super::super::around::Around;
use super::super::lock::{Acquired, Held, SetupMutex};
use super::super::machine::{MachineError, ProfileDir};
use super::super::outcome::cause_of;
use super::super::owner::Owner;
use super::super::record_values::CopyOwner;
use super::super::windows_machine::{Acting, Names};
use super::super::{admin_member, args, ffi, preflight, processes, profile, record_write};
use super::{FolderOwner, Removal, acting_token, owners};
use crate::windows::acl::object::Object;
use crate::windows::acl_plan::in_folder;
use crate::windows::apply::Surroundings;
use crate::windows::ledger::{self, LEDGER_FILE, Ledger, ledger_path};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::pool::MachineRecord;
use crate::windows::private_dir::resolve_folder;
use crate::windows::refresh;
use crate::windows::set_aside::forget_bookkeeping;
use crate::windows::sids::{Reserved, Sid};
use crate::windows::wfp;
use crate::windows::{FolderSupport, folder_support};

/// This Windows machine, as [`remove`](super::remove) drives it.
pub struct WindowsRemoval {
    names: Names,
    /// The set-up lock while held.
    held: Option<Held>,
    /// The user who asked (D-B2), when found.
    invoker: Option<Sid>,
    /// The invoker with a standard user's token, when one could be had (`acting_token`).
    actor: Option<Rc<Owner>>,
    /// Whether this thread acts as the invoker now (inside [`Removal::as_owner`]).
    acting: Rc<Cell<bool>>,
    /// Around the owner's matters, once [`Removal::owners_ledger`] checked the data folder.
    around: Around,
    /// The data folder [`Removal::owners_ledger`] checked, as the owner.
    checked: Option<PathBuf>,
}

impl WindowsRemoval {
    /// A machine working on `names`, for `invoker`, the verified parent's token user if found.
    pub fn new(names: Names, invoker: Option<Owner>) -> WindowsRemoval {
        let sid = invoker.as_ref().map(|owner| owner.sid().clone());
        let actor = invoker.and_then(|owner| acting_token(owner, Owner::elevated, Owner::lowered));
        WindowsRemoval {
            names,
            held: None,
            invoker: sid,
            actor: actor.map(Rc::new),
            acting: Rc::new(Cell::new(false)),
            // Replaced by the checked data folder before `forget_matter` can use it.
            around: Around::new(PathBuf::new()),
            checked: None,
        }
    }

    /// The group's name, once `group` is checked to be the pool group's SID.
    fn group_name(&self, group: &Sid) -> Result<String, MachineError> {
        match accounts::sid_of(&self.names.group, Kind::Group)? {
            Some(found) if &found == group => Ok(self.names.group.clone()),
            _ => Err(MachineError::Other(format!(
                "the group {} isn't the record's",
                self.names.group
            ))),
        }
    }
}

impl std::fmt::Debug for WindowsRemoval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsRemoval")
            .field("names", &self.names)
            .field("locked", &self.held.is_some())
            .field("invoker_found", &self.invoker.is_some())
            .field("can_act_as_owner", &self.actor.is_some())
            .finish_non_exhaustive()
    }
}

/// `error`'s kind, as a cause naming `what` and never a path.
fn kind_of(what: &str, error: &io::Error) -> MachineError {
    MachineError::Other(format!("{what} ({})", error.kind()))
}

/// The record's `data_dir`, checked as the module says, its owner (and its `sandbox` folder's)
/// held to [`owners`]' rules: its final path; `None` when the folder or `sandbox` is missing.
fn checked_data_dir(data_dir: &Path, owner: &Sid) -> Result<Option<PathBuf>, MachineError> {
    // The usage error quotes the path, which no cause here carries.
    let Ok(plain) = args::data_dir_of(Some(data_dir.as_os_str().to_owned())) else {
        return Err(MachineError::Other(
            "the record's data folder isn't a plain path".to_owned(),
        ));
    };
    let resolve = |path: &Path, what: &str| match resolve_folder(path) {
        Ok(found) => Ok(Some(found)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(kind_of(what, &error)),
    };
    let Some((folder, folder_owner)) = resolve(&plain, "the record's data folder")? else {
        return Ok(None);
    };
    let in_admins = || admin_member::in_administrators(owner).unwrap_or(false);
    owners::data_folder(&folder_owner, owner, in_admins)?;
    let sandbox = folder.join(SANDBOX_DIR);
    match resolve(&sandbox, "the data folder's sandbox folder")? {
        None => Ok(None),
        Some((_, sandbox_owner)) => {
            owners::sandbox_folder(&sandbox_owner, owner).map(|()| Some(folder))
        }
    }
}

/// The most a ledger read here may hold: 64 MiB.
const LEDGER_CAP: usize = 64 << 20;

/// The ledger in the checked data folder `folder`, trusting `trusted`; `None` when missing.
/// Its `sandbox` folder is opened without following a link and must be where `folder` says;
/// the ledger is then opened relative to that folder's handle (`Object::read_child`), so no
/// name checked is looked up again through a path, and a link, a folder or a file with another
/// name there is refused.
fn ledger_in(folder: &Path, trusted: &[Sid]) -> Result<Option<Ledger>, MachineError> {
    let sandbox_path = folder.join(SANDBOX_DIR);
    let sandbox = match Object::open_folder(&sandbox_path) {
        Ok(sandbox) => sandbox,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(kind_of("the data folder's sandbox folder", &error)),
    };
    let found = sandbox
        .normalized()
        .map(|wide| PathBuf::from(String::from_utf16_lossy(&wide).trim_start_matches(r"\\?\")))
        .map_err(|error| kind_of("the data folder's sandbox folder", &error))?;
    if sandbox.name_surrogate()
        || !(in_folder(&found, &sandbox_path) && in_folder(&sandbox_path, &found))
    {
        return Err(MachineError::Other(
            "the data folder's sandbox folder isn't where it was checked".to_owned(),
        ));
    }
    let name: Vec<u16> = LEDGER_FILE.encode_utf16().collect();
    let Some(bytes) = sandbox
        .read_child(&name, LEDGER_CAP)
        .map_err(|error| kind_of("the ledger", &error))?
    else {
        return Ok(None);
    };
    let reserved = Reserved::of_this_machine()
        .map_err(|error| MachineError::Other(format!("this machine's accounts: {error}")))?;
    ledger::from_bytes(ledger_path(folder), &bytes, trusted, &reserved)
        .map(Some)
        .map_err(|error| kind_of("the ledger", &error))
}

impl Removal for WindowsRemoval {
    fn lock_within(&mut self, wait_ms: u32) -> Result<(), MachineError> {
        if self.held.is_some() {
            return Err(MachineError::LockHeld);
        }
        let (held, acquired) = SetupMutex::open(&self.names.mutex)?.acquire(wait_ms)?;
        if acquired == Acquired::Abandoned {
            tracing::warn!("an earlier set-up ended without giving its lock back; taking it");
        }
        self.held = Some(held);
        Ok(())
    }

    fn unlock(&mut self) {
        self.held = None;
    }

    fn read_record(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        record_write::read(&self.names.record_key)
    }

    fn write_record(&mut self, record: &MachineRecord) -> Result<(), MachineError> {
        record_write::write(&self.names.record_key, record)
    }

    fn delete_record(&mut self) -> Result<(), MachineError> {
        record_write::delete(&self.names.record_key)
    }

    fn copy_owner(&mut self) -> Result<Option<CopyOwner>, MachineError> {
        record_write::read_admin(&self.names.record_key).map(|(owner, _version)| owner)
    }

    fn unregister_stop_task(&mut self) -> Result<(), MachineError> {
        for task in refresh::POOL_TASKS {
            refresh::unregister_task(task)
                .map_err(|error| MachineError::Other(format!("{}: {error}", task.name())))?;
        }
        Ok(())
    }

    fn invoker(&self) -> Option<&Sid> {
        self.invoker.as_ref()
    }

    fn can_act_as_owner(&self) -> bool {
        self.actor.is_some()
    }

    fn stop_strays(&mut self, pool: &[Sid], owner: &Sid) -> Result<usize, MachineError> {
        let stopped = processes::stop_pool_processes(pool, owner)?;
        if !stopped.ended.is_empty() {
            tracing::warn!(
                count = stopped.ended.len(),
                "processes of the sandbox users were stopped"
            );
        }
        if !stopped.failed.is_empty() || !stopped.timed_out.is_empty() {
            tracing::warn!(
                failed = stopped.failed.len(),
                timed_out = stopped.timed_out.len(),
                "processes of the sandbox users couldn't all be stopped"
            );
        }
        Ok(stopped.failed.len() + stopped.timed_out.len())
    }

    fn remove_network(&mut self, owner: &Sid) -> Result<Option<String>, MachineError> {
        let removed = wfp::remove(&self.names.wfp, Some(owner))
            .map_err(|error| MachineError::Other(error.cause()))?;
        if removed.foreign_removed > 0 {
            tracing::warn!(
                count = removed.foreign_removed,
                "removal deleted filters set-up didn't write"
            );
        }
        if let Err(cause) = &removed.firewall {
            tracing::warn!(%cause, "the sandbox's firewall rules couldn't be removed");
        }
        Ok(removed.firewall.err())
    }

    fn account_name(&mut self, sid: &Sid) -> Result<Option<String>, MachineError> {
        Ok(accounts::name_of(sid, Kind::User)?.map(|real| self.names.engine(real)))
    }

    fn profile_of(&mut self, sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
        profile::profile_of(sid)
    }

    fn in_profile(&mut self, profile: &Path, folder: &Path) -> Result<bool, MachineError> {
        profile::reached(profile, folder)
    }

    fn delete_profile(&mut self, sid: &Sid, owner: &Sid) -> Result<(), MachineError> {
        profile::delete_profile(sid, owner)
    }

    fn delete_user(&mut self, name: &str) -> Result<(), MachineError> {
        let real = self.names.real(name);
        // The sign-in screen value first: once the account is gone, its name can't be found
        // from the record's SID, so a value left after it would be left for good.
        accounts::unhide(&real)?;
        accounts::delete_user(&real)
    }

    fn program_data(&mut self) -> Result<PathBuf, MachineError> {
        ffi::program_data()
    }

    fn group(&mut self) -> Result<Option<Sid>, MachineError> {
        accounts::sid_of(&self.names.group, Kind::Group)
    }

    fn group_members(&mut self, group: &Sid) -> Result<usize, MachineError> {
        accounts::group_members(&self.group_name(group)?)
    }

    fn delete_group(&mut self, group: &Sid) -> Result<(), MachineError> {
        accounts::delete_group(&self.group_name(group)?)
    }

    fn remove_remote_logon_deny(&mut self, group: &Sid) -> Result<(), MachineError> {
        self.group_name(group)?;
        preflight::remove_right(group, DENY_REMOTE_INTERACTIVE_RIGHT)
    }

    fn as_owner<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> Result<T, MachineError> {
        if self.acting.get() {
            return Ok(f(self));
        }
        let owner = self.actor.clone().ok_or_else(|| {
            MachineError::Other("no standard token of the user who asked to act as".to_owned())
        })?;
        let _as_owner = owner.impersonate()?;
        // Dropped first: the flag clears, then the thread reverts.
        let _acting = Acting::set(&self.acting);
        Ok(f(self))
    }

    fn owners_ledger(
        &mut self,
        data_dir: &Path,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError> {
        let owned = self.as_owner(|m| {
            let owner = m
                .invoker()
                .cloned()
                .ok_or_else(|| MachineError::Other("no user who asked".to_owned()))?;
            let Some(folder) = checked_data_dir(data_dir, &owner)? else {
                return Ok(None);
            };
            m.around = Around::new(folder.clone());
            m.checked = Some(folder.clone());
            ledger_in(&folder, trusted)
        });
        owned.and_then(|inner| inner)
    }

    fn surroundings(&mut self) -> &mut dyn Surroundings {
        &mut self.around
    }

    fn forget_set_aside(&mut self, matter: &MatterId) -> Result<(), MachineError> {
        if !self.acting.get() {
            return Err(MachineError::Other(
                "a set-aside's bookkeeping is deleted only as its owner".to_owned(),
            ));
        }
        let Some(folder) = &self.checked else {
            return Ok(());
        };
        forget_bookkeeping(folder, matter)
            .map(drop)
            .map_err(|failure| {
                MachineError::Other(format!("a matter's set-aside bookkeeping: {failure}"))
            })
    }

    fn clear_markers(&mut self) -> Result<usize, MachineError> {
        if !self.acting.get() {
            return Err(MachineError::Other(
                "the unclean-session markers are removed only as their owner".to_owned(),
            ));
        }
        let Some(folder) = &self.checked else {
            return Ok(0);
        };
        clear_markers_in(folder)
    }

    fn admins_ledger(
        &mut self,
        data_dir: &Path,
        owner: &Sid,
        trusted: &[Sid],
    ) -> Result<Option<Ledger>, MachineError> {
        match checked_data_dir(data_dir, owner)? {
            Some(folder) => ledger_in(&folder, trusted),
            None => Ok(None),
        }
    }

    fn owner_in_administrators(&mut self, owner: &Sid) -> bool {
        admin_member::in_administrators(owner).unwrap_or_else(|error| {
            tracing::warn!(cause = %cause_of(&error), "the owner's groups can't be read");
            false
        })
    }

    fn admin_target(&mut self, path: &Path, owner: &Sid) -> Result<FolderOwner, MachineError> {
        let (folder, folder_owner) =
            resolve_folder(path).map_err(|error| kind_of("a matter's folder", &error))?;
        if !(in_folder(&folder, path) && in_folder(path, &folder)) {
            return Err(MachineError::Other(
                "a matter's folder isn't where the ledger says".to_owned(),
            ));
        }
        // The owner, Administrators or SYSTEM, as for the data folder: all that is done on a
        // folder that passes is taking our own pool and group entries, and our Low label, off.
        let found = owners::matter_folder(&folder_owner, owner)?;
        match folder_support(&folder) {
            Ok(FolderSupport::Ok) => Ok(found),
            Ok(other) => Err(MachineError::Other(format!(
                "a matter's folder fails the folder check ({other:?})"
            ))),
            Err(error) => Err(kind_of("a matter's folder check", &error)),
        }
    }
}

/// Removes every unclean-session marker in `folder`'s run folder, as whoever this thread acts
/// as; how many. A run folder that is missing holds none.
fn clear_markers_in(folder: &Path) -> Result<usize, MachineError> {
    use crate::windows::runner::marker::{marker_path, remove_marker, unclean_sessions};
    let other = |error: std::io::Error| {
        MachineError::Other(format!("the unclean-session markers ({})", error.kind()))
    };
    let sessions = unclean_sessions(folder).map_err(other)?;
    for session in &sessions {
        remove_marker(&marker_path(folder, &session.id)).map_err(other)?;
    }
    Ok(sessions.len())
}
