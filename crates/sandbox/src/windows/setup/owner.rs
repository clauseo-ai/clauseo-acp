//! The set-up's owner (D-B2): the Windows user who asked for the set-up, taken from the token of
//! the process that started the elevated one, never from an argument. Under over-the-shoulder
//! elevation the elevated process runs as the administrator who typed the password, so its own
//! user is the wrong owner; the parent's token user is the one who asked.
//!
//! The parent is the one Windows recorded for this process (`InheritedFromUniqueProcessId`;
//! the `AppInfo` service sets the caller as the parent of an elevated process). The app's
//! `--parent <pid>` must name that same process; the uninstaller's hook passes none, and the
//! recorded one is used. A process ID can be reused once its process has gone, so the process
//! opened under it must have been created no later than this one; one created later is another
//! process that took the ID, and is refused.
//!
//! [`Owner::impersonate`] makes the calling thread act as the owner until the guard it returns
//! drops, a panic's unwinding included. `secrets.bin` is written that way (`PR4g`), so its DPAPI
//! blobs, the file and its DACL are the owner's. The checks below are pure and tested on every
//! OS; the token calls are in `owner/token.rs` (Windows).

use super::machine::MachineError;

#[cfg(windows)]
mod token;

#[cfg(all(windows, test))]
pub(crate) use token::elevated_tests_on;

/// The process to take the owner from: `recorded`, the parent Windows recorded for this process.
/// `claimed` (the app's `--parent`) must be that process; with none, `recorded` is used. 0 (no
/// parent recorded) is refused.
fn parent_to_open(claimed: Option<u32>, recorded: u32) -> Result<u32, MachineError> {
    if recorded == 0 {
        return Err(MachineError::Other(
            "Windows recorded no parent process for the set-up".to_owned(),
        ));
    }
    match claimed {
        Some(claimed) if claimed != recorded => Err(MachineError::Other(format!(
            "the set-up was told its parent is process {claimed}, but Windows recorded {recorded}"
        ))),
        _ => Ok(recorded),
    }
}

/// Refuses a parent created after this process (`parent_created` above `own_created`, both in
/// `FILETIME` units): its ID was reused by another process once the real parent had gone. The
/// same time is allowed, since a process's creation time has the system clock's resolution.
fn check_created(parent_created: u64, own_created: u64) -> Result<(), MachineError> {
    if parent_created > own_created {
        return Err(MachineError::Other(
            "the set-up's parent process has gone and its process ID was reused".to_owned(),
        ));
    }
    Ok(())
}

/// The owner: its SID and an impersonation token for it. Its `Debug` shows the SID only.
#[cfg(windows)]
pub struct Owner {
    /// The token user of the verified parent.
    sid: crate::windows::sids::Sid,
    /// A duplicate of the parent's token, at `SecurityImpersonation`.
    token: token::Token,
}

#[cfg(windows)]
impl Owner {
    /// The owner from this process's verified parent, as the module says. `claimed` is the
    /// app's `--parent <pid>`, or `None` from the uninstaller's hook.
    pub fn of_parent(claimed: Option<u32>) -> Result<Owner, MachineError> {
        let pid = parent_to_open(claimed, token::recorded_parent()?)?;
        let process = token::Process::open(pid)?;
        check_created(process.created()?, token::own_created()?)?;
        let token = token::Token::impersonation_of(&process)?;
        let sid = token.user()?;
        Ok(Owner { sid, token })
    }

    /// This process's own user as the owner: for tests, which have no parent to trust.
    #[cfg(test)]
    pub(crate) fn of_this_process() -> Result<Owner, MachineError> {
        let token = token::Token::impersonation_of(&token::Process::current())?;
        let sid = token.user()?;
        Ok(Owner { sid, token })
    }

    /// The owner's SID: the record's `OwnerSid` and the marker's `user_sid`.
    pub fn sid(&self) -> &crate::windows::sids::Sid {
        &self.sid
    }

    /// Makes the calling thread act as the owner until the returned guard drops. Checked after:
    /// the thread's token must name the owner at `SecurityImpersonation` or above (Windows gives
    /// a weaker level, with no error, to a caller that may not impersonate); otherwise the
    /// thread goes back to itself and this fails. Not nested: a second guard on the same thread
    /// would end both on its drop.
    pub fn impersonate(&self) -> Result<Impersonating<'_>, MachineError> {
        self.token.impersonate(&self.sid)?;
        Ok(Impersonating {
            _owner: std::marker::PhantomData,
            _thread: std::marker::PhantomData,
        })
    }
}

#[cfg(windows)]
impl std::fmt::Debug for Owner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owner")
            .field("sid", &self.sid)
            .finish_non_exhaustive()
    }
}

/// The calling thread acting as the owner; dropping it (a panic's unwinding included) makes the
/// thread itself again. Bound to the thread that made it: neither `Send` nor `Sync`.
#[cfg(windows)]
#[must_use = "the thread acts as the owner only while the guard lives"]
#[derive(Debug)]
pub struct Impersonating<'o> {
    /// The owner whose token the thread holds.
    _owner: std::marker::PhantomData<&'o Owner>,
    /// A raw pointer: not `Send`, not `Sync`.
    _thread: std::marker::PhantomData<*const ()>,
}

#[cfg(windows)]
impl Drop for Impersonating<'_> {
    fn drop(&mut self) {
        if !token::revert() {
            // A thread that can't stop acting as the owner can't be trusted with any elevated
            // step after this one; nothing safe is left but to stop the process.
            std::process::abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recorded_parent_is_used_and_a_claimed_one_must_match_it() {
        assert_eq!(parent_to_open(None, 4242), Ok(4242));
        assert_eq!(parent_to_open(Some(4242), 4242), Ok(4242));
        let other = parent_to_open(Some(17), 4242).unwrap_err();
        assert!(other.to_string().contains("17"), "{other}");
        assert!(other.to_string().contains("4242"), "{other}");
        assert!(parent_to_open(None, 0).is_err());
        assert!(parent_to_open(Some(0), 0).is_err());
    }

    #[test]
    fn a_parent_created_after_this_process_is_a_reused_id() {
        assert_eq!(check_created(100, 200), Ok(()));
        assert_eq!(check_created(200, 200), Ok(()));
        let reused = check_created(201, 200).unwrap_err();
        assert!(reused.to_string().contains("reused"), "{reused}");
    }

    /// Windows CI, unelevated: the runner's parent (nextest) runs as the same user.
    #[cfg(windows)]
    mod windows {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        use super::super::token::{elevated_tests_on, thread_has_token};
        use super::*;
        use crate::windows::record::current_user_sid;

        #[test]
        fn the_owner_is_the_parents_token_user_and_a_wrong_parent_is_refused() {
            let owner = Owner::of_parent(None).unwrap();
            assert_eq!(owner.sid(), &current_user_sid().unwrap());
            assert!(!format!("{owner:?}").contains("token"), "{owner:?}");
            let wrong = Owner::of_parent(Some(std::process::id())).unwrap_err();
            assert!(wrong.to_string().contains("parent"), "{wrong}");
        }

        #[test]
        fn the_guard_acts_as_the_owner_and_reverts_on_drop_and_on_panic() {
            let owner = Owner::of_this_process().unwrap();
            assert!(!thread_has_token());
            let guard = owner.impersonate().unwrap();
            assert!(thread_has_token());
            assert_eq!(&current_user_sid().unwrap(), owner.sid());
            drop(guard);
            assert!(!thread_has_token());
            let unwound = catch_unwind(AssertUnwindSafe(|| {
                let _guard = owner.impersonate().unwrap();
                assert!(thread_has_token());
                panic!("a step failed while acting as the owner");
            }));
            assert!(unwound.is_err());
            assert!(!thread_has_token(), "the guard reverted during the unwind");
        }

        #[test]
        #[ignore = "elevated: CI's elevated step only (CLAUSEO_ELEVATED_TESTS=1)"]
        fn elevated_the_owner_from_the_parent_is_impersonated_and_reverted() {
            if !elevated_tests_on() {
                eprintln!("skipped: needs an elevated token and CLAUSEO_ELEVATED_TESTS=1");
                return;
            }
            let owner = Owner::of_parent(None).unwrap();
            let guard = owner.impersonate().unwrap();
            assert_eq!(&current_user_sid().unwrap(), owner.sid());
            drop(guard);
            assert!(!thread_has_token());
        }
    }
}
