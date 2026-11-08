//! What the Git trees delete and record, over [`TreeFs`]: step 7 of `git_tree.rs` (the trees
//! no run uses, [`retire_all`]), the record one value at a time ([`write_record`]), `--remove`'s
//! deletion ([`remove`]) and `--adopt`'s check of `git\` ([`check_adopted`]). Every delete is
//! `discard.rs`'s plain one. Pure: tested on every OS with the fake.

use std::path::{Path, PathBuf};

use super::build::intact;
use super::discard::{discard, empty_trash};
use super::{
    Account, BUILDING, GIT_DIR, Inspected, Owner, Recorded, SOURCE_DIR, TRASH_DIR, TreeFs,
    folder_name_ok, marker_text,
};
use crate::windows::admin_copy::{self, Found};
use crate::windows::sids::Sid;

/// Deletes every tree and source copy (the whole `%ProgramFiles%\Clauseo\Sandbox\git\`) and
/// the record's subkey, as `--remove` does; either missing is `Ok`. The folder's path comes
/// from `%ProgramFiles%` alone: no admin copy need be there. Best effort for the files: what a
/// running process holds waits in the trash, scheduled for deletion at the next restart, and
/// is logged, never returned. Only the record's failure is.
pub fn remove(fs: &mut impl TreeFs) -> Result<(), String> {
    match fs.program_files() {
        Ok(program_files) => {
            remove_folder(fs, &admin_copy::admin_dir(&program_files).join(GIT_DIR));
        }
        Err(why) => tracing::warn!(%why, "the Git trees' folder couldn't be found to delete"),
    }
    fs.delete_record()
}

/// [`remove`]'s files: everything in the trees' folder `git`, then `git`, by plain deletes;
/// what stays is scheduled for deletion at the next restart. A reinstall before then that
/// builds a tree at a scheduled path loses its files and its marker at the restart, so the next
/// run builds it again. The uninstaller's clear of the admin folder deletes `git\` by this too,
/// never by a take-back. Never fails.
pub(crate) fn remove_folder(fs: &mut impl TreeFs, git: &Path) {
    for name in fs.entries(git).unwrap_or_default() {
        if !name.eq_ignore_ascii_case(TRASH_DIR) {
            discard(fs, git, &git.join(&name));
        }
    }
    empty_trash(fs, git);
    if let Err(why) = fs.remove_tree(git) {
        tracing::warn!(%why, "Git trees in use stay until the restart");
        fs.log(
            false,
            "some sandbox Git files are in use; they are deleted at the next restart",
        );
        if let Err(why) = fs.delete_at_reboot(git) {
            tracing::warn!(%why, "the Git trees left couldn't be scheduled for deletion");
        }
    }
}

/// Writes the record, one value per account, each replaced whole by one write; then deletes
/// the values of accounts no longer in `values`. Nothing is deleted before it is replaced, so
/// a reader never finds an account's value missing. The first failure stops it.
pub(super) fn write_record(fs: &mut impl TreeFs, values: &[(Sid, String)]) -> Result<(), String> {
    for (sid, text) in values {
        fs.set_record_value(sid, text)?;
    }
    for name in fs.record_names()? {
        if !values
            .iter()
            .any(|(sid, _)| sid.as_str().eq_ignore_ascii_case(&name))
        {
            fs.delete_record_value(&name)?;
        }
    }
    Ok(())
}

/// Step 7, once the record names only current trees: each account's old trees, the folders of
/// accounts no longer in the pool, the older source copies, and the trash. `git` is the trees'
/// folder when this run found it, `current` the tree id it built.
pub(super) fn retire_all(
    fs: &mut impl TreeFs,
    git: Option<PathBuf>,
    current: Option<&str>,
    accounts: &[Account],
    outcomes: &[(Sid, Recorded)],
) {
    let git = match git {
        Some(git) => git,
        None => match fs.admin_dir() {
            Ok(admin) => admin.join(GIT_DIR),
            Err(_why) => return,
        },
    };
    for (account, (_, recorded)) in accounts.iter().zip(outcomes) {
        if folder_name_ok(&account.name) {
            let current = matches!(recorded, Recorded::Tree(_))
                .then_some(current)
                .flatten();
            retire(fs, &git, &account.name, &account.sid, current);
        }
    }
    for name in fs.entries(&git).unwrap_or_default() {
        let pool = accounts
            .iter()
            .any(|account| account.name.eq_ignore_ascii_case(&name));
        if pool || name.eq_ignore_ascii_case(SOURCE_DIR) || name.eq_ignore_ascii_case(TRASH_DIR) {
            continue;
        }
        // A dropped SID's account is gone, and a rolled-back user was deleted: no run of
        // theirs goes on.
        discard(fs, &git, &git.join(&name));
    }
    // The trees' links keep the files of an older copy, so it goes at once; so does the
    // current one when it isn't SYSTEM's or its marker is wrong, for the next build to make.
    let sources = git.join(SOURCE_DIR);
    for name in fs.entries(&sources).unwrap_or_default() {
        let copy = sources.join(&name);
        let keep = current
            .filter(|id| name.eq_ignore_ascii_case(id))
            .is_some_and(|id| intact(fs, &copy, &marker_text(id, None)).unwrap_or(false));
        if !keep {
            discard(fs, &git, &copy);
        }
    }
    empty_trash(fs, &git);
}

/// In `git\<name>\`, the account `account`'s: every temporary tree deleted; `current` and the
/// previous tree (the newest other) kept; every other tree deleted once no process runs as
/// the account (one that can't be told counts as running).
fn retire(fs: &mut impl TreeFs, git: &Path, name: &str, account: &Sid, current: Option<&str>) {
    let folder = git.join(name);
    let Ok(names) = fs.entries(&folder) else {
        return;
    };
    let mut trees = Vec::new();
    for name in names {
        if current.is_some_and(|id| name.eq_ignore_ascii_case(id)) {
            continue;
        }
        if name.ends_with(BUILDING) {
            discard(fs, git, &folder.join(&name));
            continue;
        }
        let made = match fs.inspect(&folder.join(&name)) {
            Ok(Some(found)) => found.created,
            Ok(None) => continue,
            Err(_why) => 0,
        };
        trees.push((made, name));
    }
    // The previous tree: a launch that read the record before this run wrote it starts there.
    let previous = trees.iter().max().map(|(_, name)| name.clone());
    let mut active = None;
    for (_, name) in trees {
        if previous.as_ref() == Some(&name) {
            continue;
        }
        if *active.get_or_insert_with(|| fs.active(account).unwrap_or(true)) {
            return;
        }
        discard(fs, git, &folder.join(&name));
    }
}

/// `--adopt`'s check of the trees' folder `git` (`admin_copy::take_back::lock_down_with`, which
/// leaves `git\` out of its lock-down): at every level the trees make (`git\`, what is in it,
/// and what is in each of those), anything that isn't a plain folder or file owned by SYSTEM or
/// Administrators, with a DACL only administrators can change, is deleted by a plain delete.
/// The next set-up or refresh builds again what went. Missing is fine; never fails.
pub fn check_adopted(fs: &mut impl TreeFs, git: &Path) {
    match fs.inspect(git) {
        Ok(None) => return,
        Ok(Some(found)) if trusted(&found) && found.kind == Found::Folder => {}
        Ok(Some(found)) => {
            tracing::warn!(
                ?found,
                "--adopt: the Git trees' folder isn't administrators'; deleted"
            );
            if let Err(why) = fs.remove_tree(git) {
                tracing::warn!(%why, "--adopt: some of the Git trees' folder stays");
            }
            return;
        }
        Err(why) => {
            tracing::warn!(%why, "--adopt: the Git trees' folder couldn't be read");
            return;
        }
    }
    for name in fs.entries(git).unwrap_or_default() {
        let child = git.join(&name);
        if !check_level(fs, git, &child) {
            continue;
        }
        for below in fs.entries(&child).unwrap_or_default() {
            check_level(fs, git, &child.join(&below));
        }
    }
}

/// One level of [`check_adopted`]: deleted unless [`trusted`]. Whether it is a folder kept.
fn check_level(fs: &mut impl TreeFs, git: &Path, path: &Path) -> bool {
    match fs.inspect(path) {
        Ok(Some(found)) if trusted(&found) => found.kind == Found::Folder,
        Ok(None) => false,
        Ok(Some(_)) | Err(_) => {
            tracing::warn!("--adopt: a Git tree's folder isn't administrators'; it is deleted");
            discard(fs, git, path);
            false
        }
    }
}

/// Whether `found` is a plain folder or file owned by SYSTEM or Administrators, with a DACL only
/// administrators can change.
fn trusted(found: &Inspected) -> bool {
    matches!(found.kind, Found::Folder | Found::File)
        && matches!(found.owner, Owner::System | Owner::Administrators)
        && found.admin_only
}
