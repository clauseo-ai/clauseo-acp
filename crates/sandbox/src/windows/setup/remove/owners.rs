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

/// Who owns a matter folder that passed [`matter_folder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderOwner {
    /// The record's owner.
    Owners,
    /// Administrators or SYSTEM: a folder the owner may not have made, such as `ProgramData`'s.
    Elevated,
}

/// Whether `found` may own a matter folder an administrator takes entries off for `owner`,
/// and which of the allowed owners it is. On an [`FolderOwner::Elevated`] folder only the pool's
/// entries come off; the Low label too only for an owner in Administrators ([`label_off`]).
pub(in crate::windows::setup) fn matter_folder(
    found: &Sid,
    owner: &Sid,
) -> Result<FolderOwner, MachineError> {
    held(owner_allowed(found, owner), "a matter's folder")?;
    Ok(if found == owner {
        FolderOwner::Owners
    } else {
        FolderOwner::Elevated
    })
}

/// Whether an administrator may take the recorded Low label off a matter folder `folder` owns
/// for an absent owner. The ledger is the owner's to write, so on a folder the owner doesn't
/// own a crafted one could name another program's folder and strip its Low label, which that
/// program's Low processes write by. Only for an owner in Administrators (`owner_in_admins`,
/// asked only then), who could take it off anyway.
pub(in crate::windows::setup) fn label_off(
    folder: FolderOwner,
    owner_in_admins: impl FnOnce() -> bool,
) -> bool {
    folder == FolderOwner::Owners || owner_in_admins()
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
    fn a_matter_folder_is_the_owners_or_elevated() {
        let owner = Sid::parse(OWNER).unwrap();
        let of = |found: &str| matter_folder(&Sid::parse(found).unwrap(), &owner).unwrap();
        assert_eq!(of(OWNER), FolderOwner::Owners);
        assert_eq!(of("S-1-5-32-544"), FolderOwner::Elevated);
        assert_eq!(of("S-1-5-18"), FolderOwner::Elevated);
    }

    #[test]
    fn the_low_label_comes_off_an_elevated_folder_only_for_an_owner_in_administrators() {
        assert!(label_off(FolderOwner::Owners, || panic!("not asked")));
        assert!(label_off(FolderOwner::Elevated, || true));
        assert!(!label_off(FolderOwner::Elevated, || false));
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
