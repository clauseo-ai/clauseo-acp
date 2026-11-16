//! The folder `\Clauseo` the SYSTEM tasks live in, as `register_task` (`task.rs`) prepares it,
//! on every OS through [`Folders`]: one there already is reused only when an administrator made
//! it, else deleted with everything in it and made again with the tasks' descriptor.
//!
//! A standard user may be able to make `\Clauseo` before Clauseo does: the Task Scheduler checks
//! the caller's right to make a folder under the root (MS-TSCH, `SchRpcCreateFolder`), and who
//! holds that right isn't documented. Such a folder is owned by that user. Setting the descriptor
//! again would take the folder back, but would leave whatever that user put in it (a task of
//! theirs, a folder below). The owner tells the two apart: only an administrator can make
//! `Administrators` or `SYSTEM` the owner.
//!
//! An owner that can't be read is treated as anyone else's: the folder is deleted. That is safe,
//! since a folder Clauseo made holds only Clauseo's tasks, which `register_task` registers
//! again; keeping a folder whose owner is unknown could keep a user's tasks in it.
//!
//! Two outcomes follow, both accepted. A user-made `\Clauseo` holding a task administrators
//! can't delete fails the delete, and so the registration (`--adopt` or the set-up) with that
//! error: a user who could make the folder can block the install, but can't take it over. A
//! folder an administrator made whose owner is that administrator's own SID, not
//! `Administrators`, is deleted and made again, which loses nothing.

/// The Task Scheduler's folders, as the preparation uses them; `task.rs` implements it over
/// `ITaskService` and the tests fake it. Every error is a line for the log.
pub(super) trait Folders {
    /// A folder, open.
    type Folder;
    /// `\Clauseo`, when it is there.
    fn find(&mut self) -> Result<Option<Self::Folder>, String>;
    /// `folder`'s descriptor, its owner alone, as SDDL (`O:BA`).
    fn owner_sddl(&mut self, folder: &Self::Folder) -> Result<String, String>;
    /// Deletes every task in `folder` and in each folder below it, those folders, then it.
    fn delete(&mut self, folder: Self::Folder) -> Result<(), String>;
    /// Makes `\Clauseo` with `sddl`, its owner and DACL.
    fn create(&mut self, sddl: &str) -> Result<Self::Folder, String>;
    /// Sets `sddl` on `folder`, its owner and DACL.
    fn set(&mut self, folder: &Self::Folder, sddl: &str) -> Result<(), String>;
}

/// The owner an SDDL string names (`O:<owner>`, up to the group, DACL or SACL part); `None` with
/// no owner part.
pub(super) fn sddl_owner(sddl: &str) -> Option<&str> {
    let rest = sddl.strip_prefix("O:")?;
    let end = ["G:", "D:", "S:"]
        .iter()
        .filter_map(|part| rest.find(part))
        .min()
        .unwrap_or(rest.len());
    rest.get(..end).filter(|owner| !owner.is_empty())
}

/// Whether `owner` (as SDDL writes it) is `Administrators` or `SYSTEM`: an owner only an
/// administrator, or the system, can give a folder.
pub(super) fn admin_owner(owner: &str) -> bool {
    matches!(owner, "BA" | "SY" | "S-1-5-32-544" | "S-1-5-18")
}

/// `\Clauseo`, ready for the tasks: one owned by `Administrators` or `SYSTEM` gets `sddl`
/// again; any other, or one whose owner can't be read, is deleted with everything in it; a
/// missing or deleted one is made with `sddl`.
pub(super) fn prepare<F: Folders>(folders: &mut F, sddl: &str) -> Result<F::Folder, String> {
    if let Some(folder) = folders.find()? {
        let owner = folders.owner_sddl(&folder).ok();
        if owner
            .as_deref()
            .and_then(sddl_owner)
            .is_some_and(admin_owner)
        {
            folders.set(&folder, sddl)?;
            return Ok(folder);
        }
        folders.delete(folder)?;
    }
    folders.create(sddl)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDDL: &str = "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;S-1-5-21-1-2-3-1001)";

    /// The folder `\Clauseo` as the fake has it: its owner, when it is there.
    #[derive(Default)]
    struct Fake {
        there: Option<String>,
        fail_delete: bool,
        fail_owner: bool,
        calls: Vec<String>,
    }

    impl Folders for Fake {
        type Folder = ();

        fn find(&mut self) -> Result<Option<()>, String> {
            self.calls.push("find".to_owned());
            Ok(self.there.as_ref().map(|_| ()))
        }

        fn owner_sddl(&mut self, (): &()) -> Result<String, String> {
            self.calls.push("owner".to_owned());
            if self.fail_owner {
                return Err("ITaskFolder::GetSecurityDescriptor failed (0x80070005)".to_owned());
            }
            Ok(format!("O:{}", self.there.clone().unwrap_or_default()))
        }

        fn delete(&mut self, (): ()) -> Result<(), String> {
            self.calls.push("delete".to_owned());
            if self.fail_delete {
                return Err("ITaskFolder::DeleteTask failed (0x80070005)".to_owned());
            }
            self.there = None;
            Ok(())
        }

        fn create(&mut self, sddl: &str) -> Result<(), String> {
            self.calls.push(format!("create {sddl}"));
            self.there = sddl_owner(sddl).map(str::to_owned);
            Ok(())
        }

        fn set(&mut self, (): &(), sddl: &str) -> Result<(), String> {
            self.calls.push(format!("set {sddl}"));
            Ok(())
        }
    }

    fn prepared(there: Option<&str>) -> Fake {
        let mut fake = Fake {
            there: there.map(str::to_owned),
            ..Fake::default()
        };
        prepare(&mut fake, SDDL).unwrap();
        fake
    }

    #[test]
    fn a_missing_folder_is_made_with_the_tasks_descriptor() {
        let fake = prepared(None);
        assert_eq!(fake.calls, ["find".to_owned(), format!("create {SDDL}")]);
    }

    #[test]
    fn a_folder_an_administrator_made_is_kept_and_given_the_descriptor_again() {
        for owner in ["BA", "SY", "S-1-5-32-544", "S-1-5-18"] {
            let fake = prepared(Some(owner));
            let expected = ["find".to_owned(), "owner".to_owned(), format!("set {SDDL}")];
            assert_eq!(fake.calls, expected, "{owner}");
        }
    }

    #[test]
    fn a_folder_a_standard_user_made_is_deleted_and_made_again() {
        // A local user, the record owner themselves, and the built-in Users group.
        for owner in ["S-1-5-21-1-2-3-1001", "S-1-5-21-1-2-3-1013", "BU", ""] {
            let fake = prepared(Some(owner));
            let expected = [
                "find".to_owned(),
                "owner".to_owned(),
                "delete".to_owned(),
                format!("create {SDDL}"),
            ];
            assert_eq!(fake.calls, expected, "{owner}");
            assert_eq!(fake.there.as_deref(), Some("BA"));
        }
    }

    #[test]
    fn a_folder_whose_owner_cant_be_read_is_deleted_and_made_again() {
        let mut fake = Fake {
            there: Some("BA".to_owned()),
            fail_owner: true,
            ..Fake::default()
        };
        prepare(&mut fake, SDDL).unwrap();
        let expected = [
            "find".to_owned(),
            "owner".to_owned(),
            "delete".to_owned(),
            format!("create {SDDL}"),
        ];
        assert_eq!(fake.calls, expected);
    }

    #[test]
    fn a_folder_that_wont_be_deleted_fails_and_makes_nothing() {
        let mut fake = Fake {
            there: Some("S-1-5-21-1-2-3-1013".to_owned()),
            fail_delete: true,
            ..Fake::default()
        };
        let error = prepare(&mut fake, SDDL).unwrap_err();
        assert!(error.contains("DeleteTask"), "{error}");
        assert!(!fake.calls.iter().any(|call| call.starts_with("create")));
        assert!(!fake.calls.iter().any(|call| call.starts_with("set")));
    }

    #[test]
    fn the_owner_is_read_up_to_the_next_part() {
        assert_eq!(sddl_owner("O:BA"), Some("BA"));
        assert_eq!(sddl_owner("O:BAG:SYD:P(A;;FA;;;SY)"), Some("BA"));
        assert_eq!(
            sddl_owner("O:S-1-5-21-1-2-3-1013D:(A;;FA;;;WD)"),
            Some("S-1-5-21-1-2-3-1013")
        );
        assert_eq!(sddl_owner("O:SYS:(AU;;FA;;;WD)"), Some("SY"));
        assert_eq!(sddl_owner("D:P(A;;FA;;;SY)"), None);
        assert_eq!(sddl_owner("O:D:P"), None);
        assert!(!admin_owner("BU") && !admin_owner("S-1-5-32-545") && !admin_owner("SYX"));
    }
}
