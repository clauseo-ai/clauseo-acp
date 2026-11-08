//! `HKLM\SOFTWARE\Clauseo`, the parent of every Clauseo machine key, secured before set-up uses
//! it. Set-up first makes the record's key under it, and a key that Windows makes takes its
//! parent's default security, which can let Users make subkeys there. So [`ensure`] opens the
//! key without following a link and refuses one, sets its pinned owner and protected DACL
//! ([`VENDOR_SDDL`]), then walks what is under it: a subkey whose owner isn't SYSTEM or
//! Administrators, and any link, is deleted through its handle, children first, and Clauseo's own
//! subkeys (`Sandbox`) are set back to their pinned security. The walk is pure here, against
//! [`Tree`]; `os.rs` is the registry, and [`secure`] is set-up's entry.
//! Tested on every OS against a fake tree.

#[cfg(windows)]
use super::MachineError;
use super::record_key::VENDOR_KEY;
use super::record_values::RECORD_SDDL;
#[cfg(windows)]
use crate::windows::acl::privilege::{Privileges, RECLAIM};

#[cfg(windows)]
mod os;

/// The pinned security of `HKLM\SOFTWARE\Clauseo`: owned by SYSTEM, a protected DACL giving
/// SYSTEM and Administrators full control and Users read, and no subkey for anyone else. The
/// `OI` and `CI` ACEs reach a subkey made under it without an SD of its own.
pub(super) const VENDOR_SDDL: &str = "O:SYD:P(A;OICI;KA;;;SY)(A;OICI;KA;;;BA)(A;OICI;KR;;;BU)";

/// The owners a key may have: SYSTEM and Administrators, by alias or SID.
const TRUSTED_OWNERS: [&str; 4] = ["SY", "BA", "S-1-5-18", "S-1-5-32-544"];

/// The name of the record's key under the vendor key (`record_write.rs`).
const RECORD_NAME: &str = "Sandbox";

/// How deep the walk goes; a deeper tree is refused rather than walked.
const MAX_DEPTH: usize = 32;

/// Why `HKLM\SOFTWARE\Clauseo` couldn't be secured; set-up stops on any of them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum VendorFault {
    /// The key is a registry link, which set-up never follows or repairs.
    #[error("it is a registry link")]
    Link,
    /// The key couldn't be opened or read.
    #[error("it couldn't be opened: {0}")]
    Open(String),
    /// Its owner or DACL, or one of Clauseo's keys' security, couldn't be set.
    #[error("its security couldn't be set: {0}")]
    Secure(String),
    /// A key under it couldn't be read or deleted.
    #[error("a key under it couldn't be read or removed: {0}")]
    CleanUp(String),
    /// The keys under it nest deeper than the walk goes.
    #[error("the keys under it nest too deep to check")]
    TooDeep,
}

/// What [`ensure`] changed, for the log.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Report {
    /// The security the key had when it was weaker than [`VENDOR_SDDL`], as SDDL.
    pub(super) weaker_from: Option<String>,
    /// The keys deleted, by path under `HKLM`.
    pub(super) removed: Vec<String>,
    /// The keys whose pinned security was set back, by path under `HKLM`.
    pub(super) resecured: Vec<String>,
}

/// The registry under `HKLM\SOFTWARE\Clauseo`, through handles: a key opened by name from its
/// parent's handle is never followed through a link, and a key is deleted through its handle.
/// Errors are the Windows call's text.
pub(super) trait Tree {
    /// An open key.
    type Key;
    /// The vendor key opened without following a link; `None` when it is missing.
    fn open_root(&mut self) -> Result<Option<Self::Key>, String>;
    /// Makes the vendor key with [`VENDOR_SDDL`] as its security.
    fn create_root(&mut self) -> Result<(), String>;
    /// Whether the key is a registry link.
    fn is_link(&mut self, key: &Self::Key) -> Result<bool, String>;
    /// The key's owner and DACL as SDDL, `O:..D:..`.
    fn sddl(&mut self, key: &Self::Key) -> Result<String, String>;
    /// Sets the key's owner and protected DACL to those of `sddl`.
    fn set_sddl(&mut self, key: &Self::Key, sddl: &str) -> Result<(), String>;
    /// The names of the key's subkeys.
    fn subkeys(&mut self, key: &Self::Key) -> Result<Vec<String>, String>;
    /// Opens subkey `name` of `parent` without following a link.
    fn open_child(&mut self, parent: &Self::Key, name: &str) -> Result<Self::Key, String>;
    /// Deletes the key `key` names, through its handle; it has no subkeys left.
    fn delete(&mut self, key: Self::Key) -> Result<(), String>;
}

/// The owner in `sddl`, as SDDL text: `SY`, `BA`, or a SID; empty when there is none.
fn owner_of(sddl: &str) -> &str {
    let rest = sddl.strip_prefix("O:").unwrap_or("");
    let end = ["G:", "D:", "S:"]
        .iter()
        .filter_map(|tag| rest.find(tag))
        .min();
    end.and_then(|end| rest.get(..end)).unwrap_or(rest)
}

/// The DACL in `sddl`, from `D:` to any SACL: what the protected DACL is compared by.
fn dacl_of(sddl: &str) -> &str {
    let Some(rest) = sddl.find("D:").and_then(|start| sddl.get(start..)) else {
        return "";
    };
    rest.find("S:")
        .and_then(|end| rest.get(..end))
        .unwrap_or(rest)
}

/// Whether `sddl`'s owner is SYSTEM or Administrators.
fn trusted_owner(sddl: &str) -> bool {
    TRUSTED_OWNERS.contains(&owner_of(sddl))
}

/// The pinned security of a Clauseo subkey whose SD is known, by its name.
fn pinned_child(name: &str) -> Option<String> {
    name.eq_ignore_ascii_case(RECORD_NAME)
        .then(|| format!("O:BA{RECORD_SDDL}"))
}

/// Secures the vendor key as the module says, and returns what it changed. Any fault stops it:
/// set-up must not go on under a key it couldn't secure.
pub(super) fn ensure<T: Tree>(tree: &mut T) -> Result<Report, VendorFault> {
    let mut report = Report::default();
    let (root, created) = if let Some(root) = tree.open_root().map_err(VendorFault::Open)? {
        (root, false)
    } else {
        tree.create_root().map_err(VendorFault::Secure)?;
        let root = tree
            .open_root()
            .map_err(VendorFault::Open)?
            .ok_or_else(|| VendorFault::Open("it vanished as it was made".to_owned()))?;
        (root, true)
    };
    if tree.is_link(&root).map_err(VendorFault::Open)? {
        return Err(VendorFault::Link);
    }
    let before = tree.sddl(&root).map_err(VendorFault::Open)?;
    tree.set_sddl(&root, VENDOR_SDDL)
        .map_err(VendorFault::Secure)?;
    let pinned = trusted_owner(&before) && dacl_of(&before) == dacl_of(VENDOR_SDDL);
    if !created && !pinned {
        report.weaker_from = Some(before);
    }
    clean(tree, &root, VENDOR_KEY, 1, &mut report)?;
    Ok(report)
}

/// Walks the subkeys of `parent` (`path` is its path under `HKLM`, `depth` its level): an
/// untrusted or linked subkey goes, and a subkey with a pinned SD is set back to it.
fn clean<T: Tree>(
    tree: &mut T,
    parent: &T::Key,
    path: &str,
    depth: usize,
    report: &mut Report,
) -> Result<(), VendorFault> {
    if depth > MAX_DEPTH {
        return Err(VendorFault::TooDeep);
    }
    for name in tree.subkeys(parent).map_err(VendorFault::CleanUp)? {
        let child_path = format!("{path}\\{name}");
        let child = tree
            .open_child(parent, &name)
            .map_err(VendorFault::CleanUp)?;
        let link = tree.is_link(&child).map_err(VendorFault::CleanUp)?;
        let sddl = if link {
            String::new()
        } else {
            tree.sddl(&child).map_err(VendorFault::CleanUp)?
        };
        if link || !trusted_owner(&sddl) {
            remove(tree, child, depth + 1)?;
            report.removed.push(child_path);
            continue;
        }
        if let Some(pinned) = pinned_child(&name)
            && (owner_of(&sddl) != owner_of(&pinned) || dacl_of(&sddl) != dacl_of(&pinned))
        {
            tree.set_sddl(&child, &pinned)
                .map_err(VendorFault::Secure)?;
            report.resecured.push(child_path.clone());
        }
        clean(tree, &child, &child_path, depth + 1, report)?;
    }
    Ok(())
}

/// Deletes `key` and everything under it, children first, each through its handle.
fn remove<T: Tree>(tree: &mut T, key: T::Key, depth: usize) -> Result<(), VendorFault> {
    if depth > MAX_DEPTH {
        return Err(VendorFault::TooDeep);
    }
    for name in tree.subkeys(&key).map_err(VendorFault::CleanUp)? {
        let child = tree.open_child(&key, &name).map_err(VendorFault::CleanUp)?;
        remove(tree, child, depth + 1)?;
    }
    tree.delete(key).map_err(VendorFault::CleanUp)
}

/// Secures `HKLM\SOFTWARE\Clauseo` (set-up's entry): [`ensure`] on the registry, with
/// [`RECLAIM`]'s privileges held for the walk and one log line per change. A fault is
/// [`MachineError::VendorKey`]; nothing after it runs.
#[cfg(windows)]
pub(super) fn secure() -> Result<(), MachineError> {
    let fault = |text: String| MachineError::VendorKey(text);
    let _held = Privileges::enable(&RECLAIM)
        .map_err(|error| fault(format!("its privileges weren't enabled: {error}")))?;
    let report = ensure(&mut os::Registry).map_err(|found| fault(found.to_string()))?;
    if let Some(from) = &report.weaker_from {
        tracing::warn!(was = %from, "HKLM\\SOFTWARE\\Clauseo was weaker than its pinned security");
    }
    for key in &report.removed {
        tracing::warn!(%key, "removed a key under the vendor key: not SYSTEM's or Admins', or a link");
    }
    for key in &report.resecured {
        tracing::warn!(%key, "set a Clauseo key under the vendor key back to its pinned security");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(all(windows, test))]
mod windows_tests;
