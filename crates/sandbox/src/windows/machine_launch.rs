//! The pool group denied COM launch and activation machine-wide (ADR 0049, "COM launch
//! restriction"): `HKLM\SOFTWARE\Microsoft\Ole\MachineLaunchRestriction`, a `REG_BINARY`
//! self-relative security descriptor COM checks on every launch and activation of an
//! out-of-process server, before any server's own launch permission. WSL is reached that way
//! (`WSLService`, `LxssManager`), and the agents' own tools make no out-of-process activation,
//! so a deny for `ClauseoSandboxUsers` with all five COM rights (`0x1F`, `CCDCLCSWRP`) closes
//! WSL to them whatever an `msiexec` repair does to WSL's own `AppID` keys.
//!
//! [`apply`] reads the value and keeps every entry in it exactly, adding only the deny, first.
//! The backup of the value as it was is written before the first change. A run that finds the
//! deny first already writes nothing. A value that won't parse ([`sd`]: an invalid descriptor,
//! a DACL absent or NULL, an entry breaking COM's rules) is never touched. With no value
//! (Windows' default applies), the new value is Windows' documented default plus the deny, and
//! the backup says "absent". DCOM keeps the value only beside `MachineAccessRestriction`, so a
//! missing one is first written as the access descriptor COM uses now, and the deny counts only
//! once read back after a settle ([`hold`]). A Group Policy value
//! (`HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DCOM\MachineLaunchRestriction`) overrides
//! ours: the guard then counts only when the policy's own SDDL holds the deny.
//!
//! What the guard achieved goes to the record as [`Guard::text`] (`MachineLaunchDeny`,
//! `REG_SZ`, readable by Users, so the app and firm IT can read it): `applied`, `policy`, or
//! `degraded: ...`. A degraded guard never fails the set-up: WSL's other layers stay. It is
//! never recorded as `applied`: the guard fails closed to `degraded`.
//!
//! [`remove`] takes off only our exact deny. When the value is exactly what Clauseo's first
//! write left, with no put-back since, it puts the backup back byte for byte instead, deleting
//! the value when there was none. Then the access value and `MachineLaunchRestrictionOld` are
//! cleaned of what Clauseo wrote ([`hold`]). Pure; tested on every OS.

use crate::windows::sids::Sid;

pub mod hold;
pub mod sd;
pub mod seam;

#[cfg(windows)]
mod effective;
#[cfg(test)]
mod fake;
#[cfg(windows)]
pub mod os;
#[cfg(test)]
mod test_sd;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_hold;

pub use sd::{DENY_MASK, LaunchSd, SdError};
pub use seam::{Backup, LaunchStore, Raw, StoreError};

/// The record's value holding the guard's state, [`Guard::text`].
pub const STATE_VALUE: &str = "MachineLaunchDeny";

/// What the guard achieved: the state the record holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// The value denies the pool group first, and no Group Policy overrides it.
    Applied,
    /// A Group Policy value overrides ours, and its SDDL denies the pool group first.
    ByPolicy,
    /// A Group Policy value overrides ours and doesn't deny the pool group: degraded.
    PolicyOverrides,
    /// The value doesn't parse, so it was left as it was: degraded.
    Unparsable,
    /// A registry read or write failed: degraded.
    Failed,
    /// The value didn't hold the deny when read back after the write (DCOM moved it to
    /// `MachineLaunchRestrictionOld`): degraded.
    Moved,
    /// `MachineAccessRestriction` is missing and COM's own access descriptor can't be read, so
    /// nothing was written: degraded.
    AccessUnknown,
}

impl Guard {
    /// The record's text for this state.
    pub fn text(self) -> &'static str {
        match self {
            Guard::Applied => "applied",
            Guard::ByPolicy => "policy",
            Guard::PolicyOverrides => "degraded: group policy",
            Guard::Unparsable => "degraded: unparsable",
            Guard::Failed => "degraded: failed",
            Guard::Moved => "degraded: dcom moved the value",
            Guard::AccessUnknown => "degraded: machine access unknown",
        }
    }

    /// Whether the guard doesn't hold.
    pub fn degraded(self) -> bool {
        !matches!(self, Guard::Applied | Guard::ByPolicy)
    }
}

/// What [`apply`] did to the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Nothing: the deny was first already, or the value was refused or unreadable.
    Unchanged,
    /// The deny was added, the first time (no backup was there).
    Added,
    /// The deny was put back: a backup shows Clauseo wrote it before, and it was gone (drift).
    PutBack,
}

/// What [`apply`] found and did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchApplied {
    /// The guard's state, as recorded.
    pub guard: Guard,
    /// What was written to the value.
    pub change: Change,
    /// Why the guard is degraded, or what else failed (the state's own write), in words
    /// naming no path or SID; `None` when nothing did.
    pub why: Option<String>,
}

impl LaunchApplied {
    /// The refresh's log line: `None` when nothing changed and the guard holds.
    pub fn line(&self) -> Option<String> {
        let mut line = match self.change {
            Change::Unchanged => String::new(),
            Change::Added => "the sandbox users' COM launch deny was added".to_owned(),
            Change::PutBack => "the sandbox users' COM launch deny was missing (drift) and was \
                                put back"
                .to_owned(),
        };
        if self.guard.degraded() || self.why.is_some() {
            if !line.is_empty() {
                line.push_str("; ");
            }
            line.push_str("MachineLaunchRestriction: ");
            line.push_str(self.guard.text());
            if let Some(why) = &self.why {
                line.push_str(" (");
                line.push_str(why);
                line.push(')');
            }
        }
        (!line.is_empty()).then_some(line)
    }
}

/// What [`remove`] did to the value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LaunchRemoved {
    /// Nothing named the pool group.
    #[default]
    Nothing,
    /// The value was exactly what Clauseo wrote, and the backup went back byte for byte.
    Restored,
    /// The value was deleted: the backup said there was none, and nothing else had changed.
    Deleted,
    /// This many denies for the pool group were taken off, every other entry kept.
    DeniesRemoved(usize),
    /// The value doesn't parse, so it was left as it was; why.
    Left(String),
}

/// Applies the guard for `group` as the module says, and records its state. Never fails: a
/// failure is [`Guard::Failed`], with why.
pub fn apply(store: &mut impl LaunchStore, group: &Sid) -> LaunchApplied {
    let mut whys: Vec<String> = Vec::new();
    let policy = match store.policy() {
        Ok(None) => None,
        Ok(Some(Ok(bytes))) => match LaunchSd::policy_denies_first(&bytes, group) {
            Ok(true) => Some(Guard::ByPolicy),
            Ok(false) => {
                whys.push("a Group Policy value overrides it".to_owned());
                Some(Guard::PolicyOverrides)
            }
            Err(error) => {
                whys.push(format!("a Group Policy value overrides it ({error})"));
                Some(Guard::PolicyOverrides)
            }
        },
        Ok(Some(Err(why))) => {
            whys.push(format!("a Group Policy value overrides it ({why})"));
            Some(Guard::PolicyOverrides)
        }
        Err(why) => {
            whys.push(format!("the Group Policy value can't be read: {why}"));
            Some(Guard::Failed)
        }
    };
    // The value is kept denying even under a policy, so the guard holds once the policy goes.
    let (value, change) = match apply_value(store, group) {
        Ok(change) => (Guard::Applied, change),
        Err((guard, why)) => {
            whys.push(why);
            (guard, Change::Unchanged)
        }
    };
    let guard = policy.unwrap_or(value);
    match store.state() {
        Ok(Some(held)) if held == guard.text() => {}
        Ok(_) => {
            if let Err(why) = store.write_state(guard.text()) {
                whys.push(format!("its state wasn't recorded: {why}"));
            }
        }
        Err(why) => whys.push(format!("its state can't be read: {why}")),
    }
    let why = (!whys.is_empty()).then(|| whys.join("; "));
    if guard.degraded() {
        tracing::warn!(state = guard.text(), why = ?why, "the COM launch guard is degraded");
    }
    LaunchApplied { guard, change, why }
}

/// The value made to deny `group` first; the guard's state and why when it can't be.
fn apply_value(store: &mut impl LaunchStore, group: &Sid) -> Result<Change, (Guard, String)> {
    let failed = |why: String| (Guard::Failed, why);
    let unparsable = |why: String| (Guard::Unparsable, why);
    let (current, original) = match store.read().map_err(failed)? {
        Raw::Absent => (LaunchSd::windows_default(), None),
        Raw::Binary(bytes) => {
            let parsed = LaunchSd::parse(&bytes).map_err(|error| unparsable(error.to_string()))?;
            (parsed, Some(bytes))
        }
        Raw::OtherType => {
            return Err(unparsable(
                "MachineLaunchRestriction isn't REG_BINARY".to_owned(),
            ));
        }
    };
    let merged = match current.with_deny(group) {
        Some(merged) => merged,
        None if original.is_some() => return Ok(Change::Unchanged),
        // Windows' default never denies the group; `with_deny` always adds to it.
        None => current,
    };
    let bytes = merged.to_bytes();
    if LaunchSd::parse(&bytes).ok().as_ref() != Some(&merged) {
        return Err(failed("the new value didn't read back".to_owned()));
    }
    let before = store.backup().map_err(failed)?;
    let change = if before.is_some() {
        Change::PutBack
    } else {
        Change::Added
    };
    let access = hold::access_to_add(store)?;
    // A write over an existing backup is a put-back: the value it merged into is someone
    // else's since, so the removal keeps it rather than restore the first original. An access
    // value added before stays recorded until one is added again.
    let backup = Backup {
        put_back: before.is_some(),
        access: access
            .clone()
            .or_else(|| before.as_ref().and_then(|kept| kept.access.clone())),
        original: before.map_or(original, |kept| kept.original),
        written: bytes.clone(),
    };
    store.write_backup(&backup).map_err(failed)?;
    if let Some(access) = &access {
        store.write_access(access).map_err(failed)?;
    }
    store.write(&bytes).map_err(failed)?;
    hold::verify(store, group, &bytes)?;
    Ok(change)
}

/// Takes the guard for `group` off as the module says, cleans the access value and
/// `MachineLaunchRestrictionOld` of Clauseo's bytes, then deletes the backup and the state.
/// The backup goes back byte for byte only when the value is what the first write left and no
/// put-back has happened since; otherwise only our exact deny comes off. A failed read or write
/// is the error, the backup kept for the next run.
pub fn remove(store: &mut impl LaunchStore, group: &Sid) -> Result<LaunchRemoved, StoreError> {
    let backup = store.backup()?;
    let untouched = |bytes: &[u8]| {
        backup
            .as_ref()
            .is_some_and(|kept| !kept.put_back && kept.written == bytes)
    };
    let removed = match store.read()? {
        Raw::Binary(bytes) if untouched(&bytes) => {
            if let Some(original) = backup.as_ref().and_then(|kept| kept.original.as_ref()) {
                store.write(original)?;
                LaunchRemoved::Restored
            } else {
                store.delete()?;
                LaunchRemoved::Deleted
            }
        }
        Raw::Absent => LaunchRemoved::Nothing,
        Raw::OtherType => {
            LaunchRemoved::Left("MachineLaunchRestriction isn't REG_BINARY".to_owned())
        }
        Raw::Binary(bytes) => match LaunchSd::parse(&bytes) {
            Err(error) => LaunchRemoved::Left(error.to_string()),
            Ok(current) => {
                let (kept, dropped) = current.without_denies(group);
                let was_absent = backup
                    .as_ref()
                    .is_some_and(|kept| kept.original.is_none() && !kept.put_back);
                if dropped == 0 {
                    LaunchRemoved::Nothing
                } else if was_absent && kept == LaunchSd::windows_default() {
                    store.delete()?;
                    LaunchRemoved::Deleted
                } else {
                    store.write(&kept.to_bytes())?;
                    LaunchRemoved::DeniesRemoved(dropped)
                }
            }
        },
    };
    if let LaunchRemoved::Left(why) = &removed {
        tracing::warn!(%why, "MachineLaunchRestriction was left as it was");
    }
    hold::remove_extras(store, backup.as_ref())?;
    store.delete_backup()?;
    store.delete_state()?;
    Ok(removed)
}
