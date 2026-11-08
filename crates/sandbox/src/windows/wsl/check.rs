//! The targets ([`resolve`]) and the launch check ([`check`]).

use super::record::AppId;
use super::sd::LaunchSd;
use super::seam::{ClassEntry, Permission, WslError, WslRead};
use super::{CLASSES, LXSS_MANAGER, SERVICES, WSL_SERVICE};
use crate::Blocked;

/// The targets found on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    /// Each `AppID` to deny on: those whose `LocalService` is one of [`SERVICES`], then the `AppID`
    /// of each registered class of [`CLASSES`] not among them, each once.
    pub app_ids: Vec<AppId>,
    /// Whether any of [`CLASSES`] is registered.
    pub registered: bool,
    /// How many of [`CLASSES`] are registered with no `AppID` value.
    pub no_app_id: usize,
}

/// The targets on this machine, as the module says. A read that fails is the error: the caller
/// can't know what it missed.
pub fn resolve(reg: &dyn WslRead) -> Result<Resolved, WslError> {
    let mut resolved = Resolved {
        app_ids: reg.service_app_ids(&SERVICES)?,
        ..Resolved::default()
    };
    resolved.app_ids.dedup();
    for clsid in CLASSES {
        match reg.class(clsid)? {
            ClassEntry::Missing => {}
            ClassEntry::NoAppId => {
                resolved.registered = true;
                resolved.no_app_id += 1;
            }
            ClassEntry::AppId(app_id) => {
                resolved.registered = true;
                if !resolved.app_ids.contains(&app_id) {
                    resolved.app_ids.push(app_id);
                }
            }
        }
    }
    Ok(resolved)
}

/// What a check that lets the launch go found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// No WSL here: neither service, and none of [`CLASSES`] registered.
    NoWsl,
    /// WSL is here and every target's deny comes first; how many targets.
    Denied(usize),
}

/// The launch check, as the lawyer, before a tier B launch. WSL is present when either service
/// is installed or any of [`CLASSES`] is registered (a read that fails counts as present). Then:
///
/// - a target whose value is missing (or whose key is), or whose deny for the record's group
///   is missing or comes after an allow, is [`Blocked::WslDenyMissing`];
/// - else a target that can't be read or parsed, a class of [`CLASSES`] with no `AppID`,
///   targets that can't be listed, or a packaged registration of any of [`CLASSES`] is
///   [`Blocked::WslDenyUnverifiable`], whichever WSL is here: the inbox one (`LxssManager`)
///   runs WSL2's VMs too, from Windows 10 2004 on.
///
/// With no WSL nothing is refused. A record with no group names no deny, so every target is
/// missing one.
pub fn check(reg: &dyn WslRead) -> Result<Found, Blocked> {
    let present = |name: &str| reg.service_exists(name).unwrap_or(true);
    let lifted = present(WSL_SERVICE);
    let inbox = present(LXSS_MANAGER);
    let resolved = resolve(reg);
    let registered = !matches!(&resolved, Ok(found) if !found.registered);
    if !(lifted || inbox || registered) {
        return Ok(Found::NoWsl);
    }
    let group = reg.group();
    let (mut missing, mut unverifiable) = (false, false);
    let mut targets = 0;
    match &resolved {
        Err(error) => {
            tracing::warn!(%error, "WSL's COM classes couldn't be listed");
            unverifiable = true;
        }
        Ok(found) => {
            unverifiable |= found.no_app_id > 0;
            targets = found.app_ids.len();
            for app_id in &found.app_ids {
                match reg.launch_permission(app_id) {
                    Ok(Permission::NoKey | Permission::Absent) => missing = true,
                    Ok(Permission::Bytes(bytes)) => match LaunchSd::parse(&bytes) {
                        Ok(sd) => {
                            missing |= !group.as_ref().is_some_and(|g| sd.denies_first(g));
                        }
                        Err(error) => {
                            tracing::warn!(%app_id, %error, "a WSL launch permission");
                            unverifiable = true;
                        }
                    },
                    Err(error) => {
                        tracing::warn!(%app_id, %error, "a WSL launch permission");
                        unverifiable = true;
                    }
                }
            }
        }
    }
    unverifiable |= reg.packaged_class().unwrap_or(true);
    if missing {
        return Err(Blocked::WslDenyMissing);
    }
    if unverifiable {
        return Err(Blocked::WslDenyUnverifiable);
    }
    Ok(Found::Denied(targets))
}
