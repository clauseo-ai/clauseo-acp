//! Who a per-user `--adopt` sets up (B1, over-the-shoulder elevation). The installer's tree is:
//! the outer installer, unelevated, running as the lawyer; the same installer re-run elevated
//! (`runas`, which may be an administrator's account when one typed their password); and
//! `--adopt`, which the elevated instance starts. Windows records the outer instance as the
//! elevated one's parent (`AppInfo` sets the caller as parent), so the lawyer is `--adopt`'s
//! parent's recorded parent: D-B2's rule taken one hop further.
//!
//! The outer instance also names its own user (`--owner-sid`). [`installer_owner`] takes the
//! lawyer as the outer instance's token user, and only when the tree is the installer's:
//! created strictly in order (the elevated instance before `--adopt`, the outer one before
//! that; a later one is a reused process ID), both instances running the same image, the outer
//! one's token not elevated, and its user the SID it named. The elevated instance's own user
//! is never used. Anything else refuses. Pure: tested on every OS; `Owner::of_installer` reads
//! the facts.

use std::path::PathBuf;

use super::super::machine::MachineError;
use crate::windows::admin_copy::same_path;
use crate::windows::sids::Sid;

/// What [`installer_owner`] reads of one process of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Facts {
    /// Its creation time, in `FILETIME` units.
    pub(crate) created: u64,
    /// Its image's full path.
    pub(crate) image: PathBuf,
    /// Its token's user.
    pub(crate) user: Sid,
    /// Whether its token holds more than a standard user's (`Token::elevated`).
    pub(crate) elevated: bool,
}

/// The lawyer `--adopt` sets up, from its own creation time `own_created`, its parent
/// (`instance`, the elevated installer) and that one's recorded parent (`outer`, the
/// unelevated installer), and the SID the installer named (`claimed`), as the module says.
pub(crate) fn installer_owner(
    own_created: u64,
    instance: &Facts,
    outer: &Facts,
    claimed: &Sid,
) -> Result<Sid, MachineError> {
    let refuse = |why: &str| Err(MachineError::Other(format!("--adopt's installer: {why}")));
    if instance.created >= own_created || outer.created >= instance.created {
        return refuse("a process of the tree has gone and its ID was reused");
    }
    if !same_path(&instance.image, &outer.image) {
        return refuse("the elevated process isn't the installer re-run");
    }
    if outer.elevated {
        return refuse("the outer installer is elevated, so it can't name the lawyer");
    }
    if outer.user != *claimed {
        return refuse("the outer installer's user isn't the one it named");
    }
    Ok(outer.user.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAWYER: &str = "S-1-5-21-1-2-3-1001";
    const ADMIN: &str = "S-1-5-21-1-2-3-500";
    const SETUP: &str = r"C:\Users\lawyer\Downloads\Clauseo_setup.exe";

    fn facts(created: u64, user: &str, elevated: bool) -> Facts {
        Facts {
            created,
            image: PathBuf::from(SETUP),
            user: Sid::parse(user).unwrap(),
            elevated,
        }
    }

    fn lawyer() -> Sid {
        Sid::parse(LAWYER).unwrap()
    }

    #[test]
    fn an_administrator_elevating_still_records_the_outer_installers_user() {
        let instance = facts(20, ADMIN, true);
        let outer = facts(10, LAWYER, false);
        assert_eq!(
            installer_owner(30, &instance, &outer, &lawyer()),
            Ok(lawyer())
        );
        // The lawyer's own elevation (admin approval) records the same user.
        let instance = facts(20, LAWYER, true);
        assert_eq!(
            installer_owner(30, &instance, &outer, &lawyer()),
            Ok(lawyer())
        );
    }

    #[test]
    fn a_named_sid_that_isnt_the_outer_installers_refuses() {
        let instance = facts(20, ADMIN, true);
        let outer = facts(10, LAWYER, false);
        let other = Sid::parse("S-1-5-21-1-2-3-1002").unwrap();
        assert!(installer_owner(30, &instance, &outer, &other).is_err());
        let admin = Sid::parse(ADMIN).unwrap();
        assert!(installer_owner(30, &instance, &outer, &admin).is_err());
    }

    #[test]
    fn a_tree_that_isnt_the_installers_refuses() {
        let outer = facts(10, LAWYER, false);
        let instance = facts(20, ADMIN, true);
        // Another image as the elevated parent (a shell, a script host).
        let shell = Facts {
            image: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            ..instance.clone()
        };
        assert!(installer_owner(30, &shell, &outer, &lawyer()).is_err());
        // An elevated outer instance can't stand for the lawyer.
        let elevated_outer = facts(10, LAWYER, true);
        assert!(installer_owner(30, &instance, &elevated_outer, &lawyer()).is_err());
        // Created out of order: a reused process ID.
        assert!(installer_owner(20, &instance, &outer, &lawyer()).is_err());
        assert!(installer_owner(30, &instance, &facts(20, LAWYER, false), &lawyer()).is_err());
        assert!(installer_owner(30, &instance, &facts(25, LAWYER, false), &lawyer()).is_err());
    }
}
