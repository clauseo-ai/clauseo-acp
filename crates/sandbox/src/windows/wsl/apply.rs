//! The elevated runs: [`apply`] (the set-up and the refresh) and [`remove`] (`--remove`).

use super::check::resolve;
use super::record::AppId;
use super::sd::LaunchSd;
use super::seam::{Permission, WslError, WslWrite};
use crate::windows::sids::Sid;

/// What an [`apply`] did. Names `AppID`s, never a path or a SID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WslApplied {
    /// How many targets it found.
    pub targets: usize,
    /// How many already had the deny first.
    pub unchanged: usize,
    /// How many values it changed.
    pub written: usize,
    /// How many values it made, from the default, because they were missing.
    pub created: usize,
    /// Each target it couldn't deny on, and why; the others are done.
    pub failed: Vec<String>,
}

impl WslApplied {
    /// Whether every target found has the deny first now.
    pub fn complete(&self) -> bool {
        self.failed.is_empty()
    }

    /// The refresh's Event Log line for it: `Err` naming each failure, a line when it changed
    /// anything, `None` when every deny was there. Names `AppID`s and counts only.
    pub fn outcome(&self) -> Result<Option<String>, String> {
        if !self.complete() {
            return Err(format!(
                "{} of {} launch permissions have no deny: {}",
                self.failed.len(),
                self.targets,
                self.failed.join("; ")
            ));
        }
        let changed = self.written + self.created;
        Ok((changed > 0).then(|| {
            format!(
                "WSL's deny for the sandbox users was put back on {changed} of {} launch \
                 permissions",
                self.targets
            )
        }))
    }
}

/// What a [`remove`] did. Names `AppID`s, never a path or a SID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WslRemoved {
    /// How many values lost a deny.
    pub cleaned: usize,
    /// How many values we had made were deleted.
    pub deleted: usize,
    /// Each value left as it was because it can't be parsed, and why.
    pub skipped: Vec<String>,
}

/// Denies `group` on every target ([`resolve`]): each value read, merged
/// ([`LaunchSd::with_deny`]), or built from the default when it is missing; the new bytes parsed
/// again and checked to be the old with the deny added before they are written; the target
/// recorded before its write, so a crash between the two leaves nothing unrecorded; the value
/// read back and checked after. A target whose key isn't there (a class names an `AppID` with
/// no key) is listed as failed, so the log says why the launch check refuses. One that fails is listed
/// in [`WslApplied::failed`] and the rest go on; a failure to find the targets or read the record
/// is the error.
pub fn apply(reg: &mut impl WslWrite, group: &Sid) -> Result<WslApplied, WslError> {
    let resolved = resolve(reg)?;
    let mut applied = WslApplied {
        targets: resolved.app_ids.len(),
        ..WslApplied::default()
    };
    for app_id in &resolved.app_ids {
        match apply_one(reg, app_id, group) {
            Ok(Outcome::NoKey) => applied.failed.push(format!(
                "{app_id}: a WSL class names this AppID, whose key isn't there"
            )),
            Ok(Outcome::Unchanged) => applied.unchanged += 1,
            Ok(Outcome::Written) => applied.written += 1,
            Ok(Outcome::Created) => applied.created += 1,
            Err(why) => applied.failed.push(format!("{app_id}: {why}")),
        }
    }
    Ok(applied)
}

/// What [`apply_one`] did.
enum Outcome {
    NoKey,
    Unchanged,
    Written,
    Created,
}

/// One target, as [`apply`] says.
fn apply_one(reg: &mut impl WslWrite, app_id: &AppId, group: &Sid) -> Result<Outcome, WslError> {
    let (sd, absent) = match reg.launch_permission(app_id)? {
        Permission::NoKey => return Ok(Outcome::NoKey),
        Permission::Absent => (default_sd(reg)?, true),
        Permission::Bytes(bytes) => (parsed(&bytes)?, false),
    };
    let mut record = reg.recorded()?;
    let merged = match sd.with_deny(group) {
        Some(merged) => merged,
        None if absent => sd.clone(),
        None => {
            if record.add(app_id, false) {
                reg.record(&record)?;
            }
            return Ok(Outcome::Unchanged);
        }
    };
    let bytes = merged.to_bytes();
    let again = parsed(&bytes)?;
    if !again.denies_first(group) || again.without_denies(group).0 != sd.without_denies(group).0 {
        return Err(WslError(
            "the merged value didn't check out, so it wasn't written".to_owned(),
        ));
    }
    if record.add(app_id, absent) {
        reg.record(&record)?;
    }
    reg.write_launch_permission(app_id, &bytes)?;
    match reg.launch_permission(app_id)? {
        Permission::Bytes(read) if parsed(&read)?.denies_first(group) => {}
        Permission::NoKey | Permission::Absent | Permission::Bytes(_) => {
            return Err(WslError("the value read back has no deny first".to_owned()));
        }
    }
    Ok(if absent {
        Outcome::Created
    } else {
        Outcome::Written
    })
}

/// The launch permission COM uses for an `AppID` with none: `DefaultLaunchPermission` when the
/// machine has one, else COM's built-in default. A malformed `DefaultLaunchPermission` is the
/// error: nothing is built from a guess.
fn default_sd(reg: &impl WslWrite) -> Result<LaunchSd, WslError> {
    match reg.default_launch_permission()? {
        Some(bytes) => LaunchSd::parse(&bytes)
            .map_err(|error| WslError(format!("DefaultLaunchPermission: {error}"))),
        None => Ok(LaunchSd::built_in_default()),
    }
}

/// `bytes` parsed, the error in words.
fn parsed(bytes: &[u8]) -> Result<LaunchSd, WslError> {
    LaunchSd::parse(bytes).map_err(|error| WslError(error.to_string()))
}

/// Takes our denies off: for each recorded target, and each target found now, every deny for
/// `group` comes off its value, which is checked and written, then read back and checked. A
/// value we made (`WslDenyAbsent`) is deleted instead once what is left is the default it was
/// made from (`DefaultLaunchPermission` as it is now, or COM's built-in). No old value is ever
/// written back. A target whose key or value is gone is done; one that can't be parsed is left
/// as it is and listed. Then the record's two values go. A read or write that fails is the
/// error, and the record stays for the next run.
pub fn remove(reg: &mut impl WslWrite, group: &Sid) -> Result<WslRemoved, WslError> {
    let record = reg.recorded()?;
    let mut targets = record.app_ids.clone();
    match resolve(reg) {
        Ok(found) => {
            for app_id in found.app_ids {
                if !targets.contains(&app_id) {
                    targets.push(app_id);
                }
            }
        }
        Err(error) => tracing::warn!(%error, "WSL's classes couldn't be listed; the record's go"),
    }
    let mut done = WslRemoved::default();
    for app_id in &targets {
        let Permission::Bytes(bytes) = reg.launch_permission(app_id)? else {
            continue;
        };
        let sd = match LaunchSd::parse(&bytes) {
            Ok(sd) => sd,
            Err(error) => {
                done.skipped.push(format!("{app_id}: {error}"));
                continue;
            }
        };
        let (rest, dropped) = sd.without_denies(group);
        if record.absent.contains(app_id) && is_default(reg, &rest)? {
            reg.delete_launch_permission(app_id)?;
            done.deleted += 1;
            continue;
        }
        if dropped == 0 {
            continue;
        }
        let bytes = rest.to_bytes();
        if parsed(&bytes)? != rest {
            return Err(WslError(format!(
                "{app_id}: the value without the deny didn't check out"
            )));
        }
        reg.write_launch_permission(app_id, &bytes)?;
        match reg.launch_permission(app_id)? {
            Permission::Bytes(read) if !parsed(&read)?.names_deny(group) => {}
            Permission::NoKey | Permission::Absent | Permission::Bytes(_) => {
                return Err(WslError(format!("{app_id}: the deny is still there")));
            }
        }
        done.cleaned += 1;
    }
    reg.forget()?;
    Ok(done)
}

/// Whether `rest` is the launch permission COM would use with no value: the machine's
/// `DefaultLaunchPermission`, or COM's built-in default.
fn is_default(reg: &impl WslWrite, rest: &LaunchSd) -> Result<bool, WslError> {
    if *rest == LaunchSd::built_in_default() {
        return Ok(true);
    }
    Ok(match reg.default_launch_permission()? {
        Some(bytes) => LaunchSd::parse(&bytes).is_ok_and(|default| default == *rest),
        None => false,
    })
}
