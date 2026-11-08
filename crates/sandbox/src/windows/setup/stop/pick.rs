//! Which processes a snapshot holds for the stop ([`pick`]): each process's user as the kernel's
//! process list, its token and its owner give it, and the rule for a process none of them names
//! ([`system_image`]). No Windows call: `windows_stopper.rs` reads, the tests fake.

use crate::windows::sids::Sid;

/// The owners a system image file may have: SYSTEM, `TrustedInstaller` and Administrators.
const SYSTEM_OWNERS: [&str; 3] = [
    "S-1-5-18",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
    "S-1-5-32-544",
];

/// The idle process's pid.
const IDLE_PID: u32 = 0;
/// The System process's pid.
const SYSTEM_PID: u32 = 4;
/// The session services run in, which no interactive logon gets.
const SERVICES_SESSION: u32 = 0;

/// One process as a snapshot saw it: its user as each source gave it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Process {
    /// Its process id.
    pub pid: u32,
    /// Its user as the kernel's process list (`WTSEnumerateProcessesExW`) gave it; `None` when
    /// the list named no SID for it (as for a process whose token's DACL shuts out SYSTEM), or
    /// didn't list it.
    pub listed: Option<Sid>,
    /// Its token's user, read through the handle; `None` when the token couldn't be read.
    pub token: Option<Sid>,
    /// Its process object's owner, read when the token couldn't be; `None` when not read.
    /// A standard user can't make another account the owner of a process, but the owner is the
    /// process's creator, which needn't be its user.
    pub owner: Option<Sid>,
    /// It was opened with `PROCESS_TERMINATE`.
    pub can_stop: bool,
    /// The kernel's process list couldn't be read for this snapshot, so `listed` is `None`
    /// for want of a list, not because the list named no one.
    pub list_failed: bool,
    /// Its session, as the kernel's process list gave it; `None` when the list didn't list it.
    pub session: Option<u32>,
    /// Its image, read when neither the token nor the owner could be.
    pub image: Image,
}

/// A process's image, as `windows_stopper.rs` read it through a query-limited handle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Image {
    /// Not read: no handle, or the token or the owner was read instead.
    #[default]
    Unread,
    /// Its Win32 path (`QueryFullProcessImageNameW`), with the image file's owner: `None`
    /// when the file couldn't be opened or its owner read, or when it is a reparse point.
    Path(String, Option<Sid>),
    /// It has no image file: a minimal process (System, Registry, Memory Compression, Secure
    /// System), which only the kernel makes.
    Missing,
    /// It has an image, but its path couldn't be read as a Win32 path.
    Failed,
}

/// Whether `image` is a system process's: one with no image file, or one whose image lies inside
/// one of `folders` (the Windows and Program Files folders, from `SHGetKnownFolderPath`) and
/// whose file is owned by SYSTEM, `TrustedInstaller` or Administrators. The owner matters because
/// the Windows folder has subfolders any user writes (`Temp`, `Tasks`, `tracing`), where a
/// file a pool user writes is that user's. Paths are compared by whole components, ASCII case
/// aside, with nothing trimmed: `C:\Windows2` and `C:\Windows ` aren't `C:\Windows`. A path
/// that isn't a plain drive-letter path, or holds `..`, matches nothing.
pub fn system_image(image: &Image, folders: &[String]) -> bool {
    match image {
        Image::Missing => true,
        Image::Path(path, owner) => {
            let system_owned = owner
                .as_ref()
                .is_some_and(|owner| SYSTEM_OWNERS.contains(&owner.as_str()));
            if !system_owned {
                return false;
            }
            let Some(path) = components(path) else {
                return false;
            };
            folders
                .iter()
                .filter_map(|folder| components(folder))
                .any(|folder| {
                    !folder.is_empty() && path.len() > folder.len() && path.starts_with(&folder)
                })
        }
        Image::Unread | Image::Failed => false,
    }
}

/// `path`'s components, ASCII-lowercased, without empty or `.` ones; `None` unless it starts
/// with a drive letter (`C:`, after an optional `\\?\`), or when one is `..`.
fn components(path: &str) -> Option<Vec<String>> {
    let path = path.replace('/', "\\");
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    let mut parts = path.split('\\');
    let drive = parts.next()?;
    let letter = drive.strip_suffix(':')?;
    if letter.len() != 1 || !letter.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut found = vec![drive.to_ascii_lowercase()];
    for part in parts.filter(|part| !part.is_empty() && *part != ".") {
        if part == ".." {
            return None;
        }
        found.push(part.to_ascii_lowercase());
    }
    Some(found)
}

/// What [`pick`] made of a process's user.
#[derive(Debug, Clone, PartialEq, Eq)]
enum User {
    /// Read: the list's SID, or the token's when the list gave none, or the owner's when
    /// neither gave one and the owner is one of the pool's SIDs.
    Known(Sid),
    /// Neither the list nor the token gave one, and the owner isn't a pool SID (or wasn't
    /// read); or the list and the token disagree.
    Unknown,
    /// Not looked at: this process, pid 0 or 4, or one with no user from any source that can't
    /// be opened to be terminated, even with `SeDebugPrivilege`, that the list puts in session
    /// 0, and whose image is a system process's ([`system_image`]). A pool user's process runs
    /// in an interactive session; one its scheduled task starts in session 0 can't have its
    /// image under the folders only administrators write.
    Exempt,
}

/// `process`'s user, as [`User`] says. The owner may add a process to the pool's, never clear
/// one: it is the process's creator, and an elevated administrator's processes, whoever runs
/// them, are owned by `Administrators`.
fn user_of(process: &Process, pool: &[Sid], own_pid: u32, folders: &[String]) -> User {
    if [own_pid, IDLE_PID, SYSTEM_PID].contains(&process.pid) {
        return User::Exempt;
    }
    let system =
        || process.session == Some(SERVICES_SESSION) && system_image(&process.image, folders);
    match (&process.listed, &process.token, &process.owner) {
        (Some(listed), Some(token), _) if listed != token => User::Unknown,
        (Some(user), _, _) | (None, Some(user), _) => User::Known(user.clone()),
        (None, None, Some(owner)) if pool.contains(owner) => User::Known(owner.clone()),
        (None, None, None) if !process.can_stop && !process.list_failed && system() => User::Exempt,
        (None, None, _) => User::Unknown,
    }
}

/// What one snapshot holds for the stop.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Picked {
    /// Each process whose user is one of the pool's SIDs, with that user, in the order seen,
    /// each pid once.
    pub stop: Vec<(u32, Sid)>,
    /// The processes whose user couldn't be read ([`User::Unknown`]).
    pub unknown: usize,
}

/// The processes of `seen` to stop, and those whose user is unknown, as [`user_of`] reads
/// each; SIDs are compared exactly. `own_pid` is this process; `folders` are the system
/// folders [`system_image`] takes.
pub fn pick(seen: &[Process], pool: &[Sid], own_pid: u32, folders: &[String]) -> Picked {
    let mut picked = Picked::default();
    let mut looked: Vec<u32> = Vec::new();
    for process in seen {
        if looked.contains(&process.pid) {
            continue;
        }
        looked.push(process.pid);
        match user_of(process, pool, own_pid, folders) {
            User::Known(user) if pool.contains(&user) => picked.stop.push((process.pid, user)),
            User::Unknown => picked.unknown += 1,
            User::Known(_) | User::Exempt => {}
        }
    }
    picked
}
