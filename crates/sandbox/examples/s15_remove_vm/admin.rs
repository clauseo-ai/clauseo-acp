//! `remove [--per-machine]` (the exe's `--remove`, started as the uninstall hook starts it) and
//! the elevated `cleanup`.

use std::path::Path;

use clauseo_sandbox::windows::pool::{POOL_GROUP, POOL_PREFIX};
use clauseo_sandbox::windows::setup::SetupExit;
use clauseo_sandbox::windows::setup::profile::{delete_profile, profile_of};
use clauseo_sandbox::windows::sids::Sid;

use crate::snapshot::{RECORD_KEY, accounts};
use crate::{OTHER_USER, Out, ROOT, win};

/// Where set-up hides the pool users from the sign-in screen.
const USER_LIST: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList";
/// `BUILTIN\Administrators`.
const ADMINS: &str = "S-1-5-32-544";

/// `remove`: the exe with `--remove` (and `--per-machine`), its parent this process, so the
/// user who asked is whoever runs this step. Prints the exit code; judging is `compare`'s.
pub(crate) fn remove(out: &mut Out, exe: &Path, per_machine: bool) -> Result<(), String> {
    if !exe.exists() {
        return Err(format!("no setup exe at {} (pass --exe)", exe.display()));
    }
    let me = win::current_user_sid()?;
    let elevated = win::elevated()?;
    out.evidence("asking as", &format!("{me}, elevated {elevated}"));
    let params = if per_machine {
        "--remove --per-machine"
    } else {
        "--remove"
    };
    out.line(&format!(
        "{} {params} (the exe raises its prompt)",
        exe.display()
    ));
    let code = win::run_exe(exe, params)?;
    let exit = SetupExit::from_code(code);
    out.evidence("exit", &format!("{code} ({exit:?})"));
    out.check(
        exit == Some(SetupExit::Removed),
        "exit 0 (Removed: everything removed, or nothing of the asker's to remove)",
    );
    Ok(())
}

fn elevated_only() -> Result<String, String> {
    if !win::elevated()? {
        return Err("run this step from an elevated PowerShell".into());
    }
    win::current_user_sid()
}

/// `cleanup`: every `ClauseoSb` account and `ClauseoVmOther` (profile, account, sign-in-screen
/// value), the group, the record and `C:\ClauseoVmX\`. Each part is tried whatever the last
/// did.
pub(crate) fn cleanup(out: &mut Out) -> Result<(), String> {
    let me = elevated_only()?;
    let me_sid = Sid::parse(&me).map_err(|e| e.to_string())?;
    let mut accounts_ok = true;
    for line in accounts()? {
        let fields: Vec<&str> = line.split('\t').collect();
        let [kind, name, sid, ..] = fields.as_slice() else {
            continue;
        };
        let ours = name.starts_with(POOL_PREFIX) || name.eq_ignore_ascii_case(OTHER_USER);
        if *kind != "user" || !ours {
            continue;
        }
        let profile = Sid::parse(sid)
            .map_err(|e| e.to_string())
            .and_then(|sid| match profile_of(&sid).map_err(|e| e.to_string())? {
                Some(_) => delete_profile(&sid, &me_sid).map_err(|e| e.to_string()),
                None => Ok(()),
            });
        let deleted = win::delete_user(name).and_then(|()| win::reg_delete_value(USER_LIST, name));
        out.evidence(name, &format!("profile {profile:?}, account {deleted:?}"));
        accounts_ok &= profile.is_ok() && deleted.is_ok();
    }
    out.check(
        accounts_ok,
        "ClauseoSb accounts, ClauseoVmOther and their profiles deleted",
    );
    let group = win::delete_group(POOL_GROUP);
    out.evidence("group", &format!("{group:?}"));
    let record = win::reg_delete_tree(RECORD_KEY);
    out.evidence("record", &format!("{record:?}"));
    out.check(
        group.is_ok() && record.is_ok(),
        "group and HKLM record deleted",
    );
    win::enable_privileges(&[
        "SeTakeOwnershipPrivilege",
        "SeRestorePrivilege",
        "SeBackupPrivilege",
    ])?;
    let root = Path::new(ROOT);
    let tree = if root.exists() {
        delete_tree(root, &me)
    } else {
        Ok(())
    };
    out.evidence(ROOT, &format!("{tree:?}"));
    out.check(tree.is_ok() && !root.exists(), &format!("{ROOT} removed"));
    Ok(())
}

/// Deletes `path` and everything below it, each item first given to Administrators with a
/// DACL that lets them and the lawyer in (the privileges let that past any DACL a fence set).
fn delete_tree(path: &Path, me: &str) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.file_type().is_symlink() {
        return if meta.is_dir() {
            win::remove_dir(path)
        } else {
            win::delete_file(path)
        };
    }
    win::set_owner(path, ADMINS)?;
    win::set_dacl(
        path,
        &format!("D:P(A;OICI;FA;;;BA)(A;OICI;FA;;;{me})"),
        true,
    )?;
    if !meta.is_dir() {
        return win::delete_file(path);
    }
    let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        delete_tree(&entry.path(), me)?;
    }
    win::remove_dir(path)
}
