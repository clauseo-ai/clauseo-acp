//! `HKLM\SOFTWARE\Clauseo`, the parent of every Clauseo machine key, secured before set-up uses
//! it. A key that Windows makes takes its parent's default security, which can let Users make
//! subkeys there, and a key an earlier run left may be owned by anyone. So [`ensure`] refuses a
//! link at the root, then takes the root over (owner SYSTEM, the protected SYSTEM and
//! Administrators DACL [`VENDOR_SDDL`]) before it walks anything. Each subkey under it is taken
//! over before its own subkeys are read: owner SYSTEM and an unprotected DACL with no ACE of its
//! own, so it inherits the root's. A link is the one thing deleted, after it is taken over and
//! given a protected SYSTEM and Administrators SD on its own handle. `Sandbox` gets its own SD
//! instead, and when its owner isn't SYSTEM or Administrators it is deleted with everything under
//! it and made again empty, since its values can't be trusted. The walk is pure here, against
//! [`Tree`]; `os.rs` is the registry, and [`secure`] is set-up's entry. Tested on every OS
//! against a fake tree.

use std::collections::VecDeque;

#[cfg(windows)]
use super::MachineError;
#[cfg(windows)]
#[cfg(windows)]
use super::record_key::VENDOR_KEY;
use super::record_values::RECORD_SDDL;
#[cfg(windows)]
use crate::windows::acl::privilege::{Privileges, RECLAIM};
use security::Security;

#[cfg(windows)]
mod os;
mod security;
#[cfg(test)]
mod tests;
#[cfg(all(windows, test))]
mod windows_tests;

/// The pinned security of `HKLM\SOFTWARE\Clauseo`: owned by SYSTEM, a protected DACL giving
/// SYSTEM and Administrators full control and Users read, and no subkey for anyone else. The
/// `OI` and `CI` ACEs reach a subkey that inherits from it.
pub(super) const VENDOR_SDDL: &str = "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)";

/// What a subkey taken over gets: owner SYSTEM and an unprotected DACL with no ACE of its own,
/// so `SetSecurityInfo` gives it the root's inheritable ACEs.
pub(super) const INHERIT_SDDL: &str = "O:SYD:AI";

/// What a link gets on its own handle before it is deleted: owner SYSTEM, and a protected DACL
/// that lets only SYSTEM and Administrators in.
pub(super) const LINK_SDDL: &str = "O:SYD:P(A;;KA;;;SY)(A;;KA;;;BA)";

/// The record's subkey under the vendor key (`record_write.rs`).
const RECORD_NAME: &str = "Sandbox";

/// The installer's subkey, `Clauseo\Clauseo`: the install folder, read on reinstall and upgrade.
/// It is taken over like any other key and never deleted; its name is not logged as an
/// admin-owned key.
const INSTALLER_NAME: &str = "Clauseo";

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

/// What [`ensure`] changed, for the log. Paths are under `HKLM`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    /// The root's SDDL when it differed structurally from [`VENDOR_SDDL`].
    pub(super) weaker_from: Option<String>,
    /// The links deleted.
    pub(super) removed: Vec<String>,
    /// The subkeys taken over, other than the pinned ones.
    pub(super) taken_over: Vec<String>,
    /// The pinned subkeys whose SD differed and was set back.
    pub(super) resecured: Vec<String>,
    /// The pinned subkeys owned by someone else, deleted with what was under them and made again
    /// empty.
    pub(super) recreated: Vec<String>,
    /// The subkeys that were owned by SYSTEM or Administrators before the take-over.
    pub(super) admin_owned: Vec<String>,
}

/// The registry under `HKLM\SOFTWARE\Clauseo`, through handles. A key is opened by name from its
/// parent's handle and never followed through a link. A key that is gone is `None`, which the
/// walk takes as done. Errors are the Windows call's text.
pub(super) trait Tree {
    /// An open key.
    type Key;
    /// The root, read, not following a link; `None` when it is missing.
    fn open_root(&mut self) -> Result<Option<Self::Key>, String>;
    /// Makes the root with [`VENDOR_SDDL`].
    fn create_root(&mut self) -> Result<(), String>;
    /// Takes the root over with `sddl`, and returns it open for writing.
    fn take_over_root(&mut self, sddl: &str) -> Result<Self::Key, String>;
    /// Subkey `name` of `parent`, read, not following a link; `None` when it is gone.
    fn open_child(&mut self, parent: &Self::Key, name: &str) -> Result<Option<Self::Key>, String>;
    /// Takes subkey `name` of `parent` over with `sddl`, and returns it open for writing; `None`
    /// when it is gone.
    fn take_over_child(
        &mut self,
        parent: &Self::Key,
        name: &str,
        sddl: &str,
    ) -> Result<Option<Self::Key>, String>;
    /// Makes subkey `name` of `parent` with `sddl`, and returns it open for writing.
    fn create_child(
        &mut self,
        parent: &Self::Key,
        name: &str,
        sddl: &str,
    ) -> Result<Self::Key, String>;
    /// Whether the key is a registry link.
    fn is_link(&mut self, key: &Self::Key) -> Result<bool, String>;
    /// The key's owner and DACL as SDDL.
    fn sddl(&mut self, key: &Self::Key) -> Result<String, String>;
    /// Sets the owner and DACL of an open key to those of `sddl`.
    fn set_sddl(&mut self, key: &Self::Key, sddl: &str) -> Result<(), String>;
    /// The names of the key's subkeys.
    fn subkeys(&mut self, key: &Self::Key) -> Result<Vec<String>, String>;
    /// Deletes the key `key` is open on, through that handle.
    fn delete(&mut self, key: Self::Key) -> Result<(), String>;
}

/// The pinned SD of a Clauseo subkey that has one.
// TODO(SandboxWatch): #749 adds `SandboxWatch` here at rebase, once it merges; it is not on main.
fn pinned_child(name: &str) -> Option<String> {
    name.eq_ignore_ascii_case(RECORD_NAME)
        .then(|| format!("O:BA{RECORD_SDDL}"))
}

/// Secures the vendor key at `path` (set-up's is [`VENDOR_KEY`]) as the module says, and returns
/// what it changed. Any fault stops it: set-up must not go on under a root it couldn't secure.
pub(super) fn ensure<T: Tree>(tree: &mut T, path: &str) -> Result<Report, VendorFault> {
    let mut report = Report::default();
    if let Some(found) = tree.open_root().map_err(VendorFault::Open)? {
        if tree.is_link(&found).map_err(VendorFault::Open)? {
            return Err(VendorFault::Link);
        }
        let before = tree.sddl(&found).map_err(VendorFault::Open)?;
        if Security::parse(&before) != Security::parse(VENDOR_SDDL) {
            report.weaker_from = Some(before);
        }
    } else {
        tree.create_root().map_err(VendorFault::Secure)?;
    }
    let root = tree
        .take_over_root(VENDOR_SDDL)
        .map_err(VendorFault::Secure)?;
    clean(tree, root, path, &mut report)?;
    Ok(report)
}

/// One key on the walk: its handle (taken over), its path, and its subkeys not yet visited.
struct Frame<K> {
    key: K,
    path: String,
    names: VecDeque<String>,
}

/// Walks every subkey under `root` with an explicit stack, depth first. Each child is taken over
/// before its own subkeys are read, so its previous owner can't add to them mid-walk. A link is
/// deleted. A pinned key owned by anyone but SYSTEM or Administrators is deleted with everything
/// under it and made again empty. Everything else is kept, taken over.
fn clean<T: Tree>(
    tree: &mut T,
    root: T::Key,
    path: &str,
    report: &mut Report,
) -> Result<(), VendorFault> {
    let names = tree.subkeys(&root).map_err(VendorFault::CleanUp)?;
    let mut stack = vec![Frame {
        key: root,
        path: path.to_owned(),
        names: names.into(),
    }];
    while let Some(frame) = stack.last_mut() {
        let Some(name) = frame.names.pop_front() else {
            stack.pop();
            continue;
        };
        let child_path = format!("{}\\{name}", frame.path);
        // What the child was before the take-over: its owner, and its SD for the pinned check.
        let before = match tree
            .open_child(&frame.key, &name)
            .map_err(VendorFault::CleanUp)?
        {
            Some(read) => Security::parse(&tree.sddl(&read).map_err(VendorFault::CleanUp)?),
            None => continue,
        };
        let pinned = pinned_child(&name);
        let target = pinned.as_deref().unwrap_or(INHERIT_SDDL);
        let Some(key) = tree
            .take_over_child(&frame.key, &name, target)
            .map_err(VendorFault::Secure)?
        else {
            continue;
        };
        if tree.is_link(&key).map_err(VendorFault::CleanUp)? {
            tree.set_sddl(&key, LINK_SDDL)
                .map_err(VendorFault::Secure)?;
            tree.delete(key).map_err(VendorFault::CleanUp)?;
            report.removed.push(child_path);
            continue;
        }
        if let Some(pinned) = pinned.as_deref()
            && !before.trusted_owner()
        {
            // A planted pinned key: its values are not trusted, so it goes and comes back empty.
            drain(tree, key, &child_path)?;
            tree.create_child(&frame.key, &name, pinned)
                .map_err(VendorFault::Secure)?;
            report.recreated.push(child_path);
            continue;
        }
        match pinned {
            Some(_) if before != Security::parse(target) => {
                report.resecured.push(child_path.clone());
            }
            Some(_) => {}
            None => {
                if before.trusted_owner() && !name.eq_ignore_ascii_case(INSTALLER_NAME) {
                    report.admin_owned.push(child_path.clone());
                }
                report.taken_over.push(child_path.clone());
            }
        }
        let names = tree.subkeys(&key).map_err(VendorFault::CleanUp)?;
        stack.push(Frame {
            key,
            path: child_path,
            names: names.into(),
        });
    }
    Ok(())
}

/// Deletes `key` and everything under it, children first. Each child is taken over before its
/// own subkeys are read, as [`clean`] does, and a child that is gone is done.
fn drain<T: Tree>(tree: &mut T, key: T::Key, path: &str) -> Result<(), VendorFault> {
    let names = tree.subkeys(&key).map_err(VendorFault::CleanUp)?;
    let mut stack = vec![Frame {
        key,
        path: path.to_owned(),
        names: names.into(),
    }];
    while let Some(frame) = stack.last_mut() {
        let Some(name) = frame.names.pop_front() else {
            if let Some(done) = stack.pop() {
                tree.delete(done.key).map_err(VendorFault::CleanUp)?;
            }
            continue;
        };
        let child_path = format!("{}\\{name}", frame.path);
        let Some(child) = tree
            .take_over_child(&frame.key, &name, INHERIT_SDDL)
            .map_err(VendorFault::Secure)?
        else {
            continue;
        };
        let names = tree.subkeys(&child).map_err(VendorFault::CleanUp)?;
        stack.push(Frame {
            key: child,
            path: child_path,
            names: names.into(),
        });
    }
    Ok(())
}

/// Secures `HKLM\SOFTWARE\Clauseo` (set-up's entry): [`ensure`] on the registry, with
/// [`RECLAIM`]'s privileges held for the walk and one log line per kind of change. A fault is
/// [`MachineError::VendorKey`]; nothing after it runs.
#[cfg(windows)]
pub(super) fn secure() -> Result<(), MachineError> {
    let fault = |text: String| MachineError::VendorKey(text);
    let _held = Privileges::enable(&RECLAIM)
        .map_err(|error| fault(format!("its privileges weren't enabled: {error}")))?;
    let report = ensure(&mut os::Registry::at(VENDOR_KEY), VENDOR_KEY)
        .map_err(|found| fault(found.to_string()))?;
    if let Some(from) = &report.weaker_from {
        tracing::warn!(was = %from, "HKLM\\SOFTWARE\\Clauseo was weaker than its pinned security");
    }
    for key in &report.removed {
        tracing::warn!(%key, "deleted a link under the vendor key");
    }
    for key in &report.recreated {
        tracing::warn!(%key, "deleted a pinned key owned by someone else and made it again empty");
    }
    for key in &report.resecured {
        tracing::warn!(%key, "set a Clauseo key under the vendor key back to its pinned security");
    }
    if !report.admin_owned.is_empty() {
        tracing::info!(
            keys = ?report.admin_owned,
            "admin-owned keys under the vendor key, taken over"
        );
    }
    if !report.taken_over.is_empty() {
        tracing::info!(
            count = report.taken_over.len(),
            "took over subkeys of the vendor key: owner SYSTEM, inheriting its DACL"
        );
    }
    Ok(())
}
