//! The task-file fallback of step 4 (`sweep.rs`), for a task the Task Scheduler won't give
//! ([`by_file`]), and whether its file can be trusted to show the task isn't the pool's
//! ([`trusted`]). The task's creator may hold write on its file, so a pool user may have
//! rewritten the principal there; the file's principal counts only once the owner the task's
//! own security descriptor names isn't a pool SID, the file's owner isn't one, and no allow
//! in the file's DACL gives a right to change it ([`WRITE_RIGHTS`]) to anyone a pool user's
//! token may hold ([`may_be_pool`]). The judgment reads a [`FileSecurity`] and makes no
//! Windows call, so it is tested on every OS; [`file_security`] reads one from the
//! descriptor's bytes.

use std::collections::BTreeMap;

use super::{Failed, Found, Names, Resolved, Sweeper, by_name, pool_owned, task_file};
use crate::windows::sids::Sid;

/// A task file as [`Sweeper::read_task_file`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFile {
    /// Its bytes, at most [`super::TASK_FILE_CAP`].
    pub bytes: Vec<u8>,
    /// Its owner and DACL, read through the handle its bytes were read through.
    pub security: FileSecurity,
}

/// What a file's security descriptor says about who may change it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSecurity {
    /// Its owner, who may always rewrite its DACL.
    pub owner: Sid,
    /// Every allow in its DACL, inherit-only ones too: the SID it names (`None` for a type that
    /// puts it elsewhere, an object or compound allow, or one that can't be read) and its mask.
    /// `None` for no DACL, or a NULL one, which lets everyone in.
    pub allows: Option<Vec<(Option<Sid>, u32)>>,
}

/// The rights that let their holder change a file, or its owner or DACL and then the file:
/// `FILE_WRITE_DATA`, `FILE_APPEND_DATA`, `FILE_WRITE_EA`, `FILE_WRITE_ATTRIBUTES`, `DELETE`,
/// `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_ALL` and `GENERIC_WRITE`.
pub const WRITE_RIGHTS: u32 =
    0x2 | 0x4 | 0x10 | 0x100 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;

/// The SIDs no pool user's token holds that a right to change the file may name: SYSTEM,
/// Administrators, LOCAL SERVICE, NETWORK SERVICE, and CREATOR OWNER and OWNER RIGHTS (which
/// stand for the owner, held apart).
const TRUSTED: [&str; 6] = [
    "S-1-5-18",
    "S-1-5-32-544",
    "S-1-5-19",
    "S-1-5-20",
    "S-1-3-0",
    "S-1-3-4",
];

/// A service's SID (`NT SERVICE\…`, `TrustedInstaller` among them): no pool user's token holds
/// one.
const SERVICE: &str = "S-1-5-80-";

/// An Entra ID account's or group's SID (`S-1-12-1-…`): a pool user is always a local
/// `S-1-5-21-<this machine>` account, so none is one, and no local token holds one but its own.
const ENTRA: &str = "S-1-12-1-";

/// Why a task the fallback read stays unreadable: its file's principal is the pool's, or may be.
const NOT_SKIPPED: &str = "the task file names a pool user or a name that resolves to none";

/// Whether the file whose security is `security` can't have been changed by a pool user of
/// `pool`: its owner isn't a pool SID, it has a DACL, and no allow there gives one of
/// [`WRITE_RIGHTS`] to a SID [`may_be_pool`] holds. `Err` with why it can't be trusted.
pub fn trusted(security: &FileSecurity, pool: &[Sid]) -> Result<(), Failed> {
    if pool.contains(&security.owner) {
        return Err("the task file's owner is a pool user".to_owned());
    }
    let Some(allows) = &security.allows else {
        return Err("the task file has no DACL, so anyone may change it".to_owned());
    };
    let open = allows
        .iter()
        .any(|(sid, mask)| mask & WRITE_RIGHTS != 0 && may_be_pool(sid.as_ref(), pool));
    if open {
        return Err(
            "the task file lets a pool user, or a group one may be in, change it".to_owned(),
        );
    }
    Ok(())
}

/// Whether a pool user's token may hold `sid` (`None`: an allow whose SID wasn't read). A local
/// account's token holds its own SID, groups of its machine, `BUILTIN` groups and well-known
/// ones (Everyone, Users, Authenticated Users, INTERACTIVE, LOCAL, its logon session's), never
/// another domain's account or group; so every SID may be, but [`TRUSTED`], a service's, an
/// Entra ID account or group (`S-1-12-1-…`: pool users are local accounts), and an account or
/// group under `S-1-5-21` whose domain is no pool SID's.
pub fn may_be_pool(sid: Option<&Sid>, pool: &[Sid]) -> bool {
    let Some(sid) = sid else {
        return true;
    };
    let text = sid.as_str();
    if TRUSTED.contains(&text) || text.starts_with(SERVICE) || text.starts_with(ENTRA) {
        return false;
    }
    match sid.domain() {
        Some(domain) => pool
            .iter()
            .any(|user| user.domain().is_none_or(|own| own == domain)),
        None => true,
    }
}

/// What the file of the task `name` in `folder`, which the Task Scheduler won't give, makes of
/// it: [`Found::Pool`] when the owner its own security descriptor names is a pool SID
/// ([`pool_owned`]), [`Found::FileSkipped`] when its file is [`trusted`] and its principal
/// resolves to a SID that isn't the pool's or is another domain's name, and [`Found::Unread`]
/// for anything else: an owner that can't be read, a file that can't be read or trusted, a
/// principal that is the pool's or resolves to nothing else.
pub(super) fn by_file<S: Sweeper>(
    fs: &mut S,
    folder: &S::Folder,
    name: &str,
    pool: &[Sid],
    names: &Names,
    resolved: &mut BTreeMap<String, Resolved>,
) -> Found {
    match pool_owned(fs, folder, name, pool) {
        Ok(true) => return Found::Pool,
        Ok(false) => {}
        Err(why) => return Found::Unread(why),
    }
    let principal = match from_file(fs, folder, name, pool) {
        Ok(principal) => principal,
        Err(why) => return Found::Unread(why),
    };
    match by_name(fs, pool, names, resolved, &principal) {
        Found::NotPool | Found::Skipped(_) => Found::FileSkipped,
        Found::Pool
        | Found::Unresolved(_)
        | Found::NoPrincipal
        | Found::Unread(_)
        | Found::FileSkipped => Found::Unread(NOT_SKIPPED.to_owned()),
    }
}

/// The principal's name the file of the task `name` in `folder` gives ([`task_file`]): its
/// path's parts checked, the file read, its security [`trusted`] and its XML read. `Err` for
/// anything that fails, or a file that names no user or group.
fn from_file<S: Sweeper>(
    fs: &mut S,
    folder: &S::Folder,
    name: &str,
    pool: &[Sid],
) -> Result<String, Failed> {
    let path = fs.folder_path(folder)?;
    principal_at(fs, &path, name, pool)
}

/// [`from_file`] for the folder whose path is `folder` (`\` the root, `\A\B` below).
fn principal_at<S: Sweeper>(
    fs: &mut S,
    folder: &str,
    name: &str,
    pool: &[Sid],
) -> Result<String, Failed> {
    let parts = task_file::parts(folder, name)?;
    let file = fs.read_task_file(&parts)?;
    trusted(&file.security, pool)?;
    let principal = task_file::principal_of(&file.bytes)?;
    principal
        .name()
        .map(str::to_owned)
        .ok_or_else(|| "the task file names no user or group".to_owned())
}

/// The [`FileSecurity`] of the self-relative security descriptor `descriptor`, as
/// `GetKernelObjectSecurity` gives it with the owner and DACL: every allow type's SID and mask
/// (the plain, compound, object, callback and callback object types). `Err` when it has no
/// owner or doesn't parse.
#[cfg(any(windows, test))]
pub fn file_security(descriptor: &[u8]) -> Result<FileSecurity, Failed> {
    use crate::windows::acl_bytes::{Acl, Descriptor, kind, mask, owner_sid, sid, sid_text};

    let malformed = |why: &dyn std::fmt::Display| {
        format!("the task file's security descriptor can't be read ({why})")
    };
    let owner = owner_sid(descriptor).ok_or_else(|| malformed(&"no owner"))?;
    let parsed = Descriptor::parse(descriptor).map_err(|error| malformed(&error))?;
    let allows = match parsed.dacl {
        None => None,
        Some(dacl) => {
            let acl = Acl::parse(dacl).map_err(|error| malformed(&error))?;
            let allows = acl
                .aces()
                .iter()
                .filter(|ace| matches!(kind(ace), 0 | 4 | 5 | 9 | 11))
                .map(|ace| (sid(ace).and_then(sid_text), mask(ace).unwrap_or(u32::MAX)));
            Some(allows.collect())
        }
    };
    Ok(FileSecurity { owner, allows })
}
