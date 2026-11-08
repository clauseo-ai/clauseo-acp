//! Who may own the folders a removal reads or changes, decided here once for the real machine
//! (`windows_removal`) and the tests' fake alike: the record's data folder, its `sandbox`
//! folder, and each matter folder an administrator takes entries off for an absent owner. Each
//! is held to the rule the sandbox folder is made and repaired by (`sids::owner_allowed`): the
//! record's owner, Administrators or SYSTEM, since an elevated process makes Administrators the
//! owner of what it creates (the built-in Administrator, or UAC off). Anyone else (a pool user,
//! another account, Everyone) could have planted what is read there, so it is refused. Causes
//! name the kind of folder, never a path or a SID.

use super::super::machine::MachineError;
use crate::windows::sids::{Sid, owner_allowed};

/// Whether `found` may own the record's data folder for its `owner`.
pub(in crate::windows::setup) fn data_folder(found: &Sid, owner: &Sid) -> Result<(), MachineError> {
    held(owner_allowed(found, owner), "the record's data folder")
}

/// Whether `found` may own the data folder's `sandbox` folder for the record's `owner`.
pub(in crate::windows::setup) fn sandbox_folder(
    found: &Sid,
    owner: &Sid,
) -> Result<(), MachineError> {
    held(owner_allowed(found, owner), "the record's data folder")
}

/// Whether `found` may own a matter folder an administrator takes entries off for `owner`.
pub(in crate::windows::setup) fn matter_folder(
    found: &Sid,
    owner: &Sid,
) -> Result<(), MachineError> {
    held(owner_allowed(found, owner), "a matter's folder")
}

fn held(allowed: bool, what: &str) -> Result<(), MachineError> {
    if allowed {
        Ok(())
    } else {
        Err(MachineError::Other(format!("{what} isn't its owner's own")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "S-1-5-21-1000000001-1000000002-1000000003-1001";
    const POOL_USER: &str = "S-1-5-21-1000000001-1000000002-1000000003-1042";
    const OTHER_USER: &str = "S-1-5-21-2000000001-2000000002-2000000003-1001";

    fn each(found: &str) -> [bool; 3] {
        let (found, owner) = (Sid::parse(found).unwrap(), Sid::parse(OWNER).unwrap());
        [
            data_folder(&found, &owner).is_ok(),
            sandbox_folder(&found, &owner).is_ok(),
            matter_folder(&found, &owner).is_ok(),
        ]
    }

    #[test]
    fn the_records_owner_may_own_each_folder() {
        assert_eq!(each(OWNER), [true; 3]);
    }

    #[test]
    fn administrators_may_own_each_folder() {
        assert_eq!(each("S-1-5-32-544"), [true; 3]);
    }

    #[test]
    fn system_may_own_each_folder() {
        assert_eq!(each("S-1-5-18"), [true; 3]);
    }

    #[test]
    fn a_pool_user_may_own_none() {
        assert_eq!(each(POOL_USER), [false; 3]);
    }

    #[test]
    fn another_user_or_everyone_may_own_none() {
        assert_eq!(each(OTHER_USER), [false; 3]);
        assert_eq!(each("S-1-1-0"), [false; 3]);
    }
}
