//! The elevated steps: `prepare` (the `C:\m` layout), `row4` (a record naming another owner,
//! and the pool profiles' owners for row 5) and `cleanup`.

use std::path::Path;

use clauseo_sandbox::windows::ledger::ledger_path;
use clauseo_sandbox::windows::setup::profile::{delete_profile, profile_of};
use clauseo_sandbox::windows::sids::Sid;
use clauseo_sandbox::windows::{sandbox_group_sid, sandbox_owner_sid, sandbox_pool_sids};

use crate::rows::{MATTERS, data_dir};
use crate::{Out, ROOT, win};

/// The machine's record, as the setup exe writes it.
const RECORD_KEY: &str = r"SOFTWARE\Clauseo\Sandbox";
/// Where set-up hides the pool users from the sign-in screen.
const USER_LIST: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon\SpecialAccounts\UserList";
/// `BUILTIN\Administrators`.
const ADMINS: &str = "S-1-5-32-544";
/// `Modify` (`0x1301bf`), `WRITE_DAC` and `WRITE_OWNER` (the Low label's write), without
/// `FILE_DELETE_CHILD`: the lawyer's entry on each matter folder `prepare` makes, inherited
/// by files and folders (`OICI`), so a child gives the lawyer `DELETE`. An owner is
/// implied `READ_CONTROL | WRITE_DAC` only; a folder the lawyer made gets `WRITE_OWNER` from
/// `CREATOR OWNER`, which these folders, made by an administrator, don't give the lawyer.
const LAWYER_ON_MATTER: &str = "0x1f01bf";
/// What `row4` plants as the record's owner: a SID no account here has.
const SOMEONE_ELSE: &str = "S-1-5-21-1000000000-1000000000-1000000000-1999";

fn elevated_only() -> Result<String, String> {
    if !win::elevated()? {
        return Err("run this step from an elevated PowerShell".into());
    }
    win::current_user_sid()
}

/// `prepare`: `C:\ClauseoVmR\m\one..four` made as an administrator, so the lawyer (the same
/// account, unelevated) has no `FILE_DELETE_CHILD` in them, only the explicit entry
/// [`LAWYER_ON_MATTER`], inheritable; and the lawyer owns `m`, where the plan writes its parent
/// and sibling entries, and each matter folder, as in the real flow. Refuses a machine already
/// set up.
pub(crate) fn prepare(out: &mut Out) -> Result<(), String> {
    let me = elevated_only()?;
    if sandbox_owner_sid().map_err(|e| e.to_string())?.is_some() {
        return Err("this machine is set up already: use a fresh VM (or `cleanup` first)".into());
    }
    let m = Path::new(ROOT).join("m");
    win::make_dir(Path::new(ROOT))?;
    win::make_dir(&m)?;
    win::set_owner(&m, &me)?;
    for matter in &MATTERS {
        let root = matter.root();
        win::make_dir(&root)?;
        // Inheritable, as on a folder the lawyer made: what the agents make below carries it.
        win::set_dacl(
            &root,
            &format!("D:(A;OICI;{LAWYER_ON_MATTER};;;{me})"),
            false,
        )?;
        // The lawyer owns each matter folder, as in the real flow (a folder the lawyer made);
        // the folder alone, nothing below it.
        win::set_owner(&root, &me)?;
        let sddl = win::sddl_of(&root)?;
        out.evidence(matter.name, &sddl);
    }
    let lawyer = |s: &str| {
        s.starts_with(&format!("O:{me}D:")) || (me.ends_with("-500") && s.starts_with("O:LAD:"))
    };
    let made = MATTERS.iter().all(|matter| {
        win::sddl_of(&matter.root()).is_ok_and(|s| lawyer(&s) && s.contains(&format!(";;;{me})")))
    });
    out.row(
        "prepare",
        made,
        "matter folders made by an administrator, owned by the lawyer, its entry on each",
    );
    Ok(())
}

/// `row4`: the record names another owner; `--reclaim` from this user exits 2 and touches
/// nothing (the ledger's bytes are the same after); the owner is put back whatever happened.
/// Then row 5's other half: no pool profile, nor its `NTUSER.DAT`, is the lawyer's.
pub(crate) fn row4(out: &mut Out, exe: &Path) -> Result<(), String> {
    let me = elevated_only()?;
    let owner = sandbox_owner_sid()
        .map_err(|e| e.to_string())?
        .ok_or("no record: run `run` first")?;
    let ledger = ledger_path(&data_dir());
    let before = std::fs::read(&ledger).unwrap_or_default();
    win::reg_set_string(RECORD_KEY, "OwnerSid", SOMEONE_ELSE)?;
    let params = format!("--reclaim --parent {}", std::process::id());
    let code = win::run_exe(exe, &params);
    let restored = win::reg_set_string(RECORD_KEY, "OwnerSid", &owner);
    let after_owner = sandbox_owner_sid().map_err(|e| e.to_string())?;
    let after = std::fs::read(&ledger).unwrap_or_default();
    out.evidence("exe --reclaim with another OwnerSid", &format!("{code:?}"));
    out.evidence(
        "OwnerSid put back",
        &format!("{restored:?}, now {after_owner:?}"),
    );
    out.evidence("ledger unchanged", &(before == after).to_string());
    let row4 = code == Ok(2)
        && restored.is_ok()
        && after_owner.as_deref() == Some(owner.as_str())
        && before == after;
    out.row(
        "4",
        row4,
        "a user who isn't OwnerSid gets exit 2 and nothing is touched",
    );
    let mut lawyers = 0;
    for sid in sandbox_pool_sids().map_err(|e| e.to_string())? {
        let Some(profile) = profile_of(&sid).map_err(|e| e.to_string())? else {
            continue;
        };
        for path in [profile.path.clone(), profile.path.join("NTUSER.DAT")] {
            let sddl = win::sddl_of(&path).unwrap_or_else(|e| format!("unreadable ({e})"));
            let lawyer = sddl.starts_with(&format!("O:{me}D:"))
                || (me.ends_with("-500") && sddl.starts_with("O:LAD:"));
            if lawyer {
                lawyers += 1;
            }
            out.evidence(&path.display().to_string(), &sddl);
        }
    }
    out.row(
        "5b",
        lawyers == 0,
        "no pool profile or NTUSER.DAT became the lawyer's",
    );
    Ok(())
}

/// `cleanup`: the pool's profiles, accounts and sign-in-screen entries, the group, the record
/// and `C:\ClauseoVmR\`. Each part is tried whatever the last did.
pub(crate) fn cleanup(out: &mut Out) -> Result<(), String> {
    let me = elevated_only()?;
    let me_sid = Sid::parse(&me).map_err(|e| e.to_string())?;
    let pool = sandbox_pool_sids().unwrap_or_default();
    let mut accounts = Vec::new();
    for sid in &pool {
        let name = win::account_name(sid.as_str());
        let profile = delete_profile(sid, &me_sid).map_err(|e| e.to_string());
        let deleted = name.clone().and_then(|n| {
            win::delete_user(&n)?;
            win::reg_delete_value(USER_LIST, &n)
        });
        out.evidence(
            sid.as_str(),
            &format!("{name:?}: profile {profile:?}, account {deleted:?}"),
        );
        accounts.push(profile.is_ok() && deleted.is_ok());
    }
    out.row(
        "cleanup accounts",
        accounts.iter().all(|ok| *ok),
        "pool profiles and accounts deleted",
    );
    let group = match sandbox_group_sid().ok().flatten() {
        Some(group) => win::account_name(&group).and_then(|name| win::delete_group(&name)),
        None => Ok(()),
    };
    out.evidence("group", &format!("{group:?}"));
    let record = win::reg_delete_tree(RECORD_KEY);
    out.evidence("record", &format!("{record:?}"));
    out.row(
        "cleanup record",
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
    out.evidence("C:\\ClauseoVmR", &format!("{tree:?}"));
    out.row(
        "cleanup folders",
        tree.is_ok() && !root.exists(),
        "C:\\ClauseoVmR removed",
    );
    Ok(())
}

/// Deletes `path` and everything below it, each item first given to Administrators with a
/// DACL that lets them and the lawyer in (the privileges let that past any DACL an agent set).
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
