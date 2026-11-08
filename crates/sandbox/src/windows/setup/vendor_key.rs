//! `HKLM\SOFTWARE\Clauseo`, the parent of every Clauseo machine key, secured before set-up uses
//! it. A key that Windows makes takes its parent's default security, which can let Users make
//! subkeys there, and a key an earlier run left may be owned by anyone. So [`ensure`]:
//!
//! 1. takes the root over with [`VENDOR_SDDL`], then checks it for a link: a key whose
//!    `SymbolicLinkValue` leads elsewhere is refused; one whose value leads back to itself only
//!    holds a fake value, which is deleted;
//! 2. walks every subkey, each taken over (owner SYSTEM, a protected SYSTEM and Administrators
//!    DACL, [`LOCKED_SDDL`]) before its own subkeys are read, and then:
//!    - a key holding a link value (a link, or a fake one) is deleted with what is under it;
//!    - a key owned by anyone but SYSTEM, Administrators or a member of Administrators was
//!      planted, and is deleted with what is under it, which also voids the planter's open
//!      handles. Only a local account (its SID in this machine's account domain) is judged by
//!      membership: a domain account's nested groups need the domain controller, so it counts
//!      as unresolved. A key whose owner is unresolved is kept, unless it is one the SYSTEM side
//!      trusts ([`pinned`]);
//!    - a pinned key whose DACL let anyone but SYSTEM and Administrators write is deleted too,
//!      even when an administrator owns it, so no handle opened through that DACL outlives it;
//!    - every other key is kept: a pinned one gets its own SD, the rest [`INHERITED_SDDL`]. Its
//!      values are kept, since only an administrator owned it, except that the installer's
//!      `Clauseo\Clauseo` whose DACL let others write gets its install folder (the uninstaller
//!      deletes there) written again from the trusted `InstallLocation` of the uninstall key, or
//!      deleted when there is none;
//!    - a pinned key that ends deleted, for any reason, is made again empty with its own SD.
//!
//! A take-over writes the owner and DACL on a handle opened with restore intent, which no DACL
//! or OWNER RIGHTS ACE can refuse. Every write is `RegSetKeySecurity` on the key's own handle,
//! which passes nothing down to the subkeys, so no write can reach through a link. A key deleted
//! under the walk is done. The walk is pure here, against [`Tree`]; `os.rs` is the registry, and
//! [`secure`] is set-up's entry. Tested on every OS against a fake tree.

use std::collections::VecDeque;

#[cfg(windows)]
use super::MachineError;
#[cfg(windows)]
use super::record_key::VENDOR_KEY;
use super::record_values::RECORD_SDDL;
#[cfg(windows)]
use crate::windows::acl::privilege::Privileges;
use security::Security;

#[cfg(windows)]
mod os;
mod security;
#[cfg(test)]
mod tests;
#[cfg(all(windows, test))]
mod windows_tests;

/// The pinned security of `HKLM\SOFTWARE\Clauseo`: owned by SYSTEM, a protected DACL giving
/// SYSTEM and Administrators full control and Users read, and no subkey for anyone else.
pub(super) const VENDOR_SDDL: &str = "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)";

/// What a key kept under the root ends with: owner SYSTEM and an unprotected DACL holding the
/// root's ACEs as inherited ones, as Windows would compute them, written out so nothing is
/// computed or propagated.
pub(super) const INHERITED_SDDL: &str =
    "O:SYD:AI(A;OICIID;KA;;;SY)(A;OICIID;KA;;;BA)(A;OICIID;KR;;;BU)";

/// What each subkey gets first, before anything under it is read: owner SYSTEM and a protected
/// DACL that lets only SYSTEM and Administrators in.
pub(super) const LOCKED_SDDL: &str = "O:SYD:P(A;;KA;;;SY)(A;;KA;;;BA)";

/// The record's subkey under the vendor key (`record_write.rs`).
const RECORD_NAME: &str = "Sandbox";

/// The installer's subkey, `Clauseo\Clauseo`: its default value is the install folder, which
/// the installer reads on repair and hands its uninstaller as the folder to delete.
const INSTALLER_NAME: &str = "Clauseo";

/// What the walk holds while it runs: `SeBackupPrivilege`, for its reads. A take-over enables
/// `SeRestorePrivilege` around its own writes only (`os.rs`).
#[cfg(windows)]
const WALK_PRIVILEGES: [&str; 1] = ["SeBackupPrivilege"];

/// Why `HKLM\SOFTWARE\Clauseo` couldn't be secured; set-up stops on any of them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum VendorFault {
    /// The root is a registry link, which set-up never follows or repairs.
    #[error("it is a registry link")]
    Link,
    /// The root couldn't be opened or read.
    #[error("it couldn't be opened: {0}")]
    Open(String),
    /// A key's owner or DACL couldn't be set, or a key couldn't be made.
    #[error("its security couldn't be set: {0}")]
    Secure(String),
    /// A key under it couldn't be read or deleted.
    #[error("a key under it couldn't be read or removed: {0}")]
    CleanUp(String),
}

/// Why a registry call on a key didn't do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Miss {
    /// The key isn't there, or was deleted while open: what was to be done to it is done.
    Gone,
    /// The call failed; the Windows call's text.
    Failed(String),
}

/// What [`ensure`] changed, for the log. Paths are under `HKLM`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    /// The root's SDDL when it differed structurally from [`VENDOR_SDDL`].
    pub(super) weaker_from: Option<String>,
    /// Whether the root held a fake link value, deleted.
    pub(super) fake_root_link: bool,
    /// The keys holding a link value, deleted.
    pub(super) removed: Vec<String>,
    /// The keys owned by someone other than an administrator, deleted.
    pub(super) planted: Vec<String>,
    /// The pinned keys deleted for any reason, made again empty.
    pub(super) recreated: Vec<String>,
    /// The pinned keys kept whose SD differed and was set back.
    pub(super) resecured: Vec<String>,
    /// The other keys kept, taken over.
    pub(super) taken_over: Vec<String>,
    /// The keys kept whose owner was an administrator other than SYSTEM and Administrators.
    pub(super) admin_owned: Vec<String>,
    /// The keys kept whose owner couldn't be resolved, with why.
    pub(super) unresolved: Vec<(String, String)>,
    /// The pinned keys whose DACL let others write, deleted (and made again).
    pub(super) writable_pinned: Vec<String>,
    /// The other keys kept whose DACL let anyone but SYSTEM and Administrators write.
    pub(super) others_could_write: Vec<String>,
    /// The installer's key whose install folder was written again from the uninstall key's
    /// `InstallLocation` (`Some`), or deleted for want of one (`None`).
    pub(super) install_dir_reset: Option<(String, Option<String>)>,
}

/// The registry under `HKLM\SOFTWARE\Clauseo`, through handles. A key is opened by name from its
/// parent's handle and never followed through a link.
pub(super) trait Tree {
    /// An open key.
    type Key;
    /// The root, read, not following a link; `None` when it is missing.
    fn open_root(&mut self) -> Result<Option<Self::Key>, Miss>;
    /// Makes the root with [`VENDOR_SDDL`]'s DACL, owned by Administrators until its take-over.
    fn create_root(&mut self) -> Result<(), Miss>;
    /// Takes the root over with `sddl`, and returns it open for writing.
    fn take_over_root(&mut self, sddl: &str) -> Result<Self::Key, Miss>;
    /// Whether the root, opened following links, is a different key from `key` (the root opened
    /// as itself): a real link. A link that leads nowhere is one.
    fn root_leads_elsewhere(&mut self, key: &Self::Key) -> Result<bool, Miss>;
    /// Subkey `name` of `parent`, read, not following a link.
    fn open_child(&mut self, parent: &Self::Key, name: &str) -> Result<Self::Key, Miss>;
    /// Takes subkey `name` of `parent` over with `sddl`, and returns it open for writing.
    fn take_over_child(
        &mut self,
        parent: &Self::Key,
        name: &str,
        sddl: &str,
    ) -> Result<Self::Key, Miss>;
    /// Makes subkey `name` of `parent` with `sddl`.
    fn create_child(&mut self, parent: &Self::Key, name: &str, sddl: &str) -> Result<(), Miss>;
    /// Whether the key holds a `SymbolicLinkValue` of type `REG_LINK`.
    fn has_link_value(&mut self, key: &Self::Key) -> Result<bool, Miss>;
    /// Deletes the key's `SymbolicLinkValue`.
    fn delete_link_value(&mut self, key: &Self::Key) -> Result<(), Miss>;
    /// The key's owner and DACL as SDDL.
    fn sddl(&mut self, key: &Self::Key) -> Result<String, Miss>;
    /// Sets the owner and DACL of an open key to those of `sddl`, passing nothing down.
    fn set_sddl(&mut self, key: &Self::Key, sddl: &str) -> Result<(), Miss>;
    /// The names of the key's subkeys.
    fn subkeys(&mut self, key: &Self::Key) -> Result<Vec<String>, Miss>;
    /// Deletes the key `key` is open on, through that handle.
    fn delete(&mut self, key: Self::Key) -> Result<(), Miss>;
    /// Writes the installer key's install folder (its default value) again from the uninstall
    /// key's `InstallLocation`, which only administrators write, and returns it; deletes the
    /// value and returns `None` when there is none.
    fn reset_install_dir(&mut self, key: &Self::Key) -> Result<Option<String>, Miss>;
    /// This machine's account domain: the three parts after `S-1-5-21` of every local account.
    fn local_domain(&mut self) -> Result<[u32; 3], String>;
    /// Whether the local account `sid` is in Administrators, directly or through a group; the
    /// error says why it couldn't be told.
    fn in_administrators(&mut self, sid: &str) -> Result<bool, String>;
}

/// The pinned SD of a key the SYSTEM side reads and trusts, by its path under the root.
// TODO(SandboxWatch, MachineLaunch): #749 adds `SandboxWatch`, and `Sandbox\MachineLaunch`
// joins once it lands; neither is on main.
fn pinned(relative: &str) -> Option<String> {
    relative
        .eq_ignore_ascii_case(RECORD_NAME)
        .then(|| format!("O:BA{RECORD_SDDL}"))
}

/// Secures the vendor key at `path` (set-up's is [`VENDOR_KEY`]) as the module says, and returns
/// what it changed. Any fault stops it: set-up must not go on under a root it couldn't secure. A
/// root deleted under the walk is walked once more from the start.
pub(super) fn ensure<T: Tree>(tree: &mut T, path: &str) -> Result<Report, VendorFault> {
    match ensure_once(tree, path)? {
        Some(report) => Ok(report),
        None => ensure_once(tree, path)?
            .ok_or_else(|| VendorFault::Open("it was deleted twice during set-up".to_owned())),
    }
}

/// One pass of [`ensure`]; `None` when the root was deleted under it.
fn ensure_once<T: Tree>(tree: &mut T, path: &str) -> Result<Option<Report>, VendorFault> {
    let mut report = Report::default();
    match tree.open_root() {
        Ok(Some(found)) => {
            let before = tree.sddl(&found).map_err(open_fault)?;
            if Security::parse(&before) != Security::parse(VENDOR_SDDL) {
                report.weaker_from = Some(before);
            }
        }
        Ok(None) | Err(Miss::Gone) => tree.create_root().map_err(secure_fault)?,
        Err(Miss::Failed(error)) => return Err(VendorFault::Open(error)),
    }
    let root = match tree.take_over_root(VENDOR_SDDL) {
        Ok(root) => root,
        Err(Miss::Gone) => return Ok(None),
        Err(Miss::Failed(error)) => return Err(VendorFault::Secure(error)),
    };
    match tree.has_link_value(&root) {
        Ok(false) => {}
        Ok(true) => {
            if tree.root_leads_elsewhere(&root).map_err(open_fault)? {
                return Err(VendorFault::Link);
            }
            tree.delete_link_value(&root).map_err(clean_fault)?;
            report.fake_root_link = true;
        }
        Err(Miss::Gone) => return Ok(None),
        Err(Miss::Failed(error)) => return Err(VendorFault::Open(error)),
    }
    let root = clean(tree, root, path, &mut report)?;
    // The root still there is the walk finished; one deleted under it is walked again.
    match tree.sddl(&root) {
        Ok(_) => Ok(Some(report)),
        Err(Miss::Gone) => Ok(None),
        Err(Miss::Failed(error)) => Err(VendorFault::Open(error)),
    }
}

fn open_fault(miss: Miss) -> VendorFault {
    VendorFault::Open(text(miss))
}

fn secure_fault(miss: Miss) -> VendorFault {
    VendorFault::Secure(text(miss))
}

fn clean_fault(miss: Miss) -> VendorFault {
    VendorFault::CleanUp(text(miss))
}

fn text(miss: Miss) -> String {
    match miss {
        Miss::Gone => "the key was deleted".to_owned(),
        Miss::Failed(error) => error,
    }
}

/// `Ok(None)` for a key that is gone, `Ok(Some)` for a result, and a fault made by `fault`.
fn unless_gone<V>(
    result: Result<V, Miss>,
    fault: fn(String) -> VendorFault,
) -> Result<Option<V>, VendorFault> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(Miss::Gone) => Ok(None),
        Err(Miss::Failed(error)) => Err(fault(error)),
    }
}

/// One key on the walk: its handle (taken over), its path under `HKLM`, its path under the root,
/// and its subkeys not yet visited.
struct Frame<K> {
    key: K,
    path: String,
    relative: String,
    names: VecDeque<String>,
}

/// What a key is judged against: its pinned SD when the SYSTEM side trusts it, and this
/// machine's account domain.
struct Judged<'a> {
    pinned: Option<&'a str>,
    local: &'a Result<[u32; 3], String>,
}

/// A key's paths: under `HKLM`, and under the root.
struct At {
    path: String,
    relative: String,
}

/// Walks every subkey under `root` with an explicit stack, depth first, as the module says, and
/// gives `root` back.
fn clean<T: Tree>(
    tree: &mut T,
    root: T::Key,
    path: &str,
    report: &mut Report,
) -> Result<T::Key, VendorFault> {
    let local = tree.local_domain();
    let names = unless_gone(tree.subkeys(&root), VendorFault::CleanUp)?.unwrap_or_default();
    let mut stack = vec![Frame {
        key: root,
        path: path.to_owned(),
        relative: String::new(),
        names: names.into(),
    }];
    loop {
        let Some(frame) = stack.last_mut() else {
            return Err(VendorFault::CleanUp(
                "the walk ended without its root".to_owned(),
            ));
        };
        let Some(name) = frame.names.pop_front() else {
            if let Some(done) = stack.pop()
                && stack.is_empty()
            {
                return Ok(done.key);
            }
            continue;
        };
        let child = At {
            path: format!("{}\\{name}", frame.path),
            relative: if frame.relative.is_empty() {
                name.clone()
            } else {
                format!("{}\\{name}", frame.relative)
            },
        };
        let pinned = pinned(&child.relative);
        let judged = Judged {
            pinned: pinned.as_deref(),
            local: &local,
        };
        let kept = visit(tree, &frame.key, &name, &child, &judged, report)?;
        match kept {
            Some(frame) => stack.push(frame),
            None => {
                if let Some(pinned) = pinned.as_deref()
                    && let Some(parent) = stack.last()
                    && unless_gone(
                        tree.create_child(&parent.key, &name, pinned),
                        VendorFault::Secure,
                    )?
                    .is_some()
                {
                    report.recreated.push(child.path);
                }
            }
        }
    }
}

/// Visits subkey `name` of `parent` (`at` gives its paths): its frame when it is kept, `None`
/// when it is gone, deleted under the walk or by it.
fn visit<T: Tree>(
    tree: &mut T,
    parent: &T::Key,
    name: &str,
    at: &At,
    judged: &Judged<'_>,
    report: &mut Report,
) -> Result<Option<Frame<T::Key>>, VendorFault> {
    let pinned = judged.pinned;
    // What the child was before the take-over.
    let Some(read) = unless_gone(tree.open_child(parent, name), VendorFault::CleanUp)? else {
        return Ok(None);
    };
    let Some(before) = unless_gone(tree.sddl(&read), VendorFault::CleanUp)? else {
        return Ok(None);
    };
    drop(read);
    let before = Security::parse(&before);
    let owner = owner_of(tree, &before.owner, judged.local);
    let Some(key) = unless_gone(
        tree.take_over_child(parent, name, LOCKED_SDDL),
        VendorFault::Secure,
    )?
    else {
        return Ok(None);
    };
    let Some(link) = unless_gone(tree.has_link_value(&key), VendorFault::CleanUp)? else {
        return Ok(None);
    };
    // The SYSTEM side's keys get no benefit of the doubt.
    let planted = match &owner {
        Owner::Planter => true,
        Owner::Unresolved(_) => pinned.is_some(),
        Owner::Admin | Owner::OtherAdmin => false,
    };
    let others_wrote = before.others_may_write();
    if link || planted || (pinned.is_some() && others_wrote) {
        drain(tree, key)?;
        let list = if link {
            &mut report.removed
        } else if planted {
            &mut report.planted
        } else {
            &mut report.writable_pinned
        };
        list.push(at.path.clone());
        return Ok(None);
    }
    let target = pinned.unwrap_or(INHERITED_SDDL);
    if unless_gone(tree.set_sddl(&key, target), VendorFault::Secure)?.is_none() {
        return Ok(None);
    }
    if others_wrote {
        report.others_could_write.push(at.path.clone());
        if at.relative.eq_ignore_ascii_case(INSTALLER_NAME) {
            let Some(reset) = unless_gone(tree.reset_install_dir(&key), VendorFault::CleanUp)?
            else {
                return Ok(None);
            };
            report.install_dir_reset = Some((at.path.clone(), reset));
        }
    }
    match owner {
        Owner::OtherAdmin => report.admin_owned.push(at.path.clone()),
        Owner::Unresolved(why) => report.unresolved.push((at.path.clone(), why)),
        Owner::Admin | Owner::Planter => {}
    }
    if pinned.is_some() {
        if before != Security::parse(target) {
            report.resecured.push(at.path.clone());
        }
    } else {
        report.taken_over.push(at.path.clone());
    }
    let names = unless_gone(tree.subkeys(&key), VendorFault::CleanUp)?.unwrap_or_default();
    Ok(Some(Frame {
        key,
        path: at.path.clone(),
        relative: at.relative.clone(),
        names: names.into(),
    }))
}

/// Who `sid`, a key's owner before the take-over, is. Only a local account is judged by
/// membership; any other is unresolved, as is everyone when the local domain can't be read.
fn owner_of<T: Tree>(tree: &mut T, sid: &str, local: &Result<[u32; 3], String>) -> Owner {
    if security::TRUSTED_OWNERS.contains(&sid) {
        return Owner::Admin;
    }
    let local = match local {
        Ok(local) => local,
        Err(why) => return Owner::Unresolved(format!("this machine's domain: {why}")),
    };
    if security::account_domain(sid).as_ref() != Some(local) {
        return Owner::Unresolved("not a local account".to_owned());
    }
    match tree.in_administrators(sid) {
        Ok(true) => Owner::OtherAdmin,
        Ok(false) => Owner::Planter,
        Err(why) => Owner::Unresolved(why),
    }
}

/// Who owned a key before the take-over.
enum Owner {
    /// SYSTEM or Administrators.
    Admin,
    /// An account in Administrators.
    OtherAdmin,
    /// Anyone else.
    Planter,
    /// An account whose membership couldn't be told, with why.
    Unresolved(String),
}

/// Deletes `key` and everything under it, bottom up through handles. Each child is taken over
/// (for `DELETE`) before its own subkeys are read; one that is gone is done.
fn drain<T: Tree>(tree: &mut T, key: T::Key) -> Result<(), VendorFault> {
    let names = unless_gone(tree.subkeys(&key), VendorFault::CleanUp)?.unwrap_or_default();
    let mut stack = vec![(key, VecDeque::from(names))];
    while let Some((parent, names)) = stack.last_mut() {
        let Some(name) = names.pop_front() else {
            if let Some((done, _)) = stack.pop() {
                unless_gone(tree.delete(done), VendorFault::CleanUp)?;
            }
            continue;
        };
        let Some(child) = unless_gone(
            tree.take_over_child(parent, &name, LOCKED_SDDL),
            VendorFault::Secure,
        )?
        else {
            continue;
        };
        let names = unless_gone(tree.subkeys(&child), VendorFault::CleanUp)?.unwrap_or_default();
        stack.push((child, names.into()));
    }
    Ok(())
}

/// Secures `HKLM\SOFTWARE\Clauseo` (set-up's entry): [`ensure`] on the registry, with
/// `SeBackupPrivilege` held for the walk and one log line per kind of change. A fault is
/// [`MachineError::VendorKey`]; nothing after it runs.
#[cfg(windows)]
pub(super) fn secure() -> Result<(), MachineError> {
    let fault = |text: String| MachineError::VendorKey(text);
    let _held = Privileges::enable(&WALK_PRIVILEGES)
        .map_err(|error| fault(format!("its privileges weren't enabled: {error}")))?;
    let report = ensure(&mut os::Registry::at(VENDOR_KEY), VENDOR_KEY)
        .map_err(|found| fault(found.to_string()))?;
    log(&report);
    Ok(())
}

/// One log line per kind of change in `report`.
#[cfg(windows)]
fn log(report: &Report) {
    if let Some(from) = &report.weaker_from {
        tracing::warn!(was = %from, "HKLM\\SOFTWARE\\Clauseo was weaker than its pinned security");
    }
    if report.fake_root_link {
        tracing::warn!("deleted a link value from the vendor key that led back to itself");
    }
    let lists = [
        (
            &report.removed,
            "deleted keys holding a link value under the vendor key",
        ),
        (
            &report.planted,
            "deleted keys under the vendor key owned by a non-administrator",
        ),
        (
            &report.recreated,
            "made Clauseo keys under the vendor key again, empty",
        ),
        (
            &report.resecured,
            "set Clauseo keys under the vendor key back to their security",
        ),
        (
            &report.writable_pinned,
            "deleted Clauseo keys under the vendor key that others could write",
        ),
        (
            &report.others_could_write,
            "kept keys under the vendor key that others could write",
        ),
    ];
    for (keys, what) in lists {
        if !keys.is_empty() {
            tracing::warn!(?keys, "{what}");
        }
    }
    match &report.install_dir_reset {
        Some((key, Some(folder))) => {
            tracing::warn!(
                %key,
                %folder,
                "wrote the installer key's folder again from the uninstall key"
            );
        }
        Some((key, None)) => {
            tracing::warn!(%key, "deleted the installer key's folder: the uninstall key has none");
        }
        None => {}
    }
    if !report.unresolved.is_empty() {
        tracing::warn!(
            keys = ?report.unresolved,
            "kept keys under the vendor key whose owner couldn't be resolved"
        );
    }
    if !report.admin_owned.is_empty() {
        tracing::info!(
            keys = ?report.admin_owned,
            "kept keys under the vendor key owned by an administrator's account"
        );
    }
    if !report.taken_over.is_empty() {
        tracing::info!(
            count = report.taken_over.len(),
            "took over subkeys of the vendor key: owner SYSTEM, the root's DACL"
        );
    }
}
