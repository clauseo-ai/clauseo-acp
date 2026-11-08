//! Git Bash for the agent (0049, "The Git tree", the runner's side): the outer runner, as the
//! desktop user, finds the run's pool account's Git tree (`git_tree`, built by set-up), checks
//! it, and makes msys's two shared-object folders for it before the agent starts.
//!
//! msys2-runtime opens `\Sessions\<session>\BaseNamedObjects\<shared id>-<key>` with
//! `OBJ_OPENIF` and puts its objects in the global `\BaseNamedObjects\<shared id>-<key>`. Under
//! the agent's token it can't make the session one; made here first, with a DACL that lets the
//! pool user in ([`object_dir_sddl`]), both open for it. [`prepare`] holds the order:
//!
//! 1. the record (`HKLM\SOFTWARE\Clauseo\Sandbox\GitTrees`, the pool SID's value): missing, an
//!    unknown value or `Git Bash unavailable: <code>` turns Git Bash off;
//! 2. the admin folder's chain (`admin_dir_checked`), then the record's path, which must be
//!    `<admin>\git\<pool account>\<tree id>` ([`tree_parts`]);
//! 3. each of `git`, `<account>` and `<tree id>` opened relative to the one above, never
//!    through a link, owner SYSTEM and exactly `git_tree`'s protected DACL;
//! 4. the files the agent starts from ([`HELD`]) opened without `FILE_SHARE_DELETE` and held for
//!    the run, so a refresh can't retire the tree between this check and the agent's first Git;
//!    a handle on a folder would hold nothing (the desktop user gets no right on the tree's
//!    folders that takes a share), and a file that can't be deleted keeps its folders;
//! 5. the tree's completion marker, `.clauseo-tree`: its tree id and SID ([`marker_ok`]);
//! 6. `usr\bin\msys-2.0.dll`: its final path must be the tree's own, and gives the key
//!    ([`msys_key`]); the file gives the shared id ([`shared_id`]);
//! 7. both folders made, never opened: one already there is squatted ([`Off::Squatted`]).
//!
//! Each failure is one plain line on stderr and no Git Bash for the run: the agent then gets
//! neither `CLAUDE_CODE_GIT_BASH_PATH` nor any Git a PATH entry of the lawyer's Git names
//! (`env::agent_env`). It never falls back to the lawyer's Git.

use std::fmt;

use super::paths::WinPath;
use crate::windows::git_tree::{BUILDING, GIT_DIR, Recorded, UNAVAILABLE};
use crate::windows::git_tree::{account_sddl, folder_name_ok, git_dir_sddl};
use crate::windows::sids::Sid;

#[cfg(windows)]
pub(crate) mod os;
#[cfg(test)]
mod tests;

/// What precedes the shared id in `msys-2.0.dll`.
pub(crate) const SHARED_ID_TAG: &[u8] = b"%%% MSYS shared id: ";
/// The tree's completion marker, at its root: `<tree id>\n<pool SID>`.
pub(crate) const MARKER: &str = ".clauseo-tree";
/// The msys runtime, from a tree's root.
pub(crate) const MSYS_DLL: [&str; 3] = ["usr", "bin", "msys-2.0.dll"];
/// The files held open for the run, from a tree's root: the runtime, Claude Code's
/// `bash.exe`, and the `git.exe` first in the agent's `PATH`.
pub(crate) const HELD: [&[&str]; 3] = [&MSYS_DLL, &["bin", "bash.exe"], &["cmd", "git.exe"]];

/// Why Git Bash is off for a run; [`fmt::Display`] is its plain line. Appended only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Off {
    /// The record has no value for the pool user.
    NotRecorded,
    /// The record says set-up couldn't make the tree, with its code.
    Unavailable(String),
    /// The record's value is neither a path nor a known code.
    Malformed,
    /// The record can't be read.
    Unreadable(String),
    /// The admin folder's chain failed its check.
    AdminFolder(String),
    /// The record's path isn't `<admin>\git\<pool account>\<tree id>`.
    Elsewhere,
    /// A level of the tree failed its check, or its marker did.
    Unverified(String),
    /// `msys-2.0.dll` couldn't be opened or read.
    Runtime(String),
    /// `msys-2.0.dll`'s final path isn't in the tree, or isn't plain ASCII.
    Moved,
    /// `msys-2.0.dll` names no valid shared id.
    NoSharedId,
    /// One of msys's two folders already exists.
    Squatted,
    /// One of msys's two folders couldn't be made.
    ObjectDir(String),
    /// A file of the tree couldn't be held open for the run.
    Unheld(String),
}

impl fmt::Display for Off {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Off::NotRecorded => write!(
                f,
                "set-up hasn't recorded a Git tree for this sandbox user; run set-up again"
            ),
            Off::Unavailable(code) => {
                write!(
                    f,
                    "set-up couldn't make this sandbox user's Git tree ({code})"
                )
            }
            Off::Malformed => write!(
                f,
                "the Git trees' record holds a value this version can't read"
            ),
            Off::Unreadable(why) => write!(f, "the Git trees' record can't be read ({why})"),
            Off::AdminFolder(why) => write!(f, "the admin folder failed its check ({why})"),
            Off::Elsewhere => write!(
                f,
                "the recorded Git tree isn't this sandbox user's folder in the admin folder"
            ),
            Off::Unverified(why) => write!(f, "the Git tree failed its check ({why})"),
            Off::Runtime(why) => write!(f, "the Git tree's msys-2.0.dll can't be read ({why})"),
            Off::Moved => write!(f, "the Git tree's msys-2.0.dll resolves outside the tree"),
            Off::NoSharedId => write!(
                f,
                "the Git tree's msys-2.0.dll names no valid MSYS shared id"
            ),
            Off::Squatted => write!(
                f,
                "msys's shared folder for this Git tree already exists: something made it first"
            ),
            Off::ObjectDir(why) => write!(f, "msys's shared folder couldn't be made ({why})"),
            Off::Unheld(why) => write!(
                f,
                "the Git tree's files couldn't be held open for the run ({why})"
            ),
        }
    }
}

/// What making an object folder found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Made {
    /// Something is there under the name already.
    Exists,
    /// Anything else, in words naming no path.
    Failed(String),
}

/// `msys-2.0.dll` as opened: its final path (`GetFinalPathNameByHandleW`, `\\?\C:\…`) and its
/// bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Runtime {
    /// The final path.
    pub(crate) final_path: String,
    /// The file's content.
    pub(crate) image: Vec<u8>,
}

/// The effects [`prepare`] needs, which `os.rs` makes as the desktop user and the tests fake.
/// Every error is words naming no path.
pub(crate) trait GitBashOs {
    /// What keeps a made folder alive; it goes when this drops.
    type Held;
    /// The record's value for `pool_user`; `None` when it has none.
    fn record(&mut self, pool_user: &Sid) -> Result<Option<String>, String>;
    /// The admin folder once its chain passes `admin_copy::check`'s rule 2.
    fn admin_dir(&mut self) -> Result<WinPath, String>;
    /// The sandbox users' group, from the machine record.
    fn group(&mut self) -> Result<Sid, String>;
    /// Opens `admin`, then each of `levels`' names below it relative to the one above, never
    /// through a reparse point, refusing unless each is a folder whose owner and DACL read
    /// back as its SDDL.
    fn check_levels(&mut self, admin: &WinPath, levels: &[(String, String)]) -> Result<(), String>;
    /// The file `path`'s text, opened as itself, a few kilobytes at most.
    fn marker(&mut self, path: &WinPath) -> Result<String, String>;
    /// The regular file `dll`, opened as itself.
    fn runtime(&mut self, dll: &WinPath) -> Result<Runtime, String>;
    /// The regular file `file`, opened as itself to read, sharing read alone (no
    /// `FILE_SHARE_DELETE`): no one can delete, rename or write it while the handle lives.
    fn hold(&mut self, file: &WinPath) -> Result<Self::Held, String>;
    /// This process's session.
    fn session(&mut self) -> Result<u32, String>;
    /// Makes the object folder `path` with the descriptor `sddl`, never opening one there.
    fn make_dir(&mut self, path: &str, sddl: &str) -> Result<Self::Held, Made>;
}

/// A run's Git Bash, ready: its tree, with its [`HELD`] files and msys's folders open for the
/// run.
#[derive(Debug)]
pub(crate) struct Ready<H> {
    /// The tree's root.
    pub(crate) tree: WinPath,
    /// The files' handles, then the folders'.
    pub(crate) held: Vec<H>,
}

/// The run's Git Bash for the pool user `pool_user` (account `pool_name`) and the desktop user
/// `desktop`, or why it is off.
pub(crate) fn prepare<O: GitBashOs>(
    os: &mut O,
    pool_user: &Sid,
    pool_name: &str,
    desktop: &Sid,
) -> Result<Ready<O::Held>, Off> {
    let text = os
        .record(pool_user)
        .map_err(Off::Unreadable)?
        .ok_or(Off::NotRecorded)?;
    if let Some(code) = text.strip_prefix(UNAVAILABLE) {
        return Err(Off::Unavailable(plain_code(code)));
    }
    let Some(Recorded::Tree(_)) = Recorded::parse(&text) else {
        return Err(Off::Malformed);
    };
    let admin = os.admin_dir().map_err(Off::AdminFolder)?;
    let tree = WinPath::parse(&text).ok_or(Off::Elsewhere)?;
    let (account, id) = tree_parts(&tree, &admin, pool_name).ok_or(Off::Elsewhere)?;
    let group = os.group().map_err(Off::Unverified)?;
    let levels = [
        (GIT_DIR.to_owned(), git_dir_sddl(&group)),
        (account, account_sddl(pool_user)),
        (id.clone(), account_sddl(pool_user)),
    ];
    os.check_levels(&admin, &levels).map_err(Off::Unverified)?;
    let mut held = Vec::new();
    for parts in HELD {
        let file = parts
            .iter()
            .fold(tree.clone(), |path, part| path.join(part));
        held.push(os.hold(&file).map_err(Off::Unheld)?);
    }
    let marker = os.marker(&tree.join(MARKER)).map_err(Off::Unverified)?;
    if !marker_ok(&marker, &id, pool_user) {
        return Err(Off::Unverified(
            "its completion marker doesn't name this tree and sandbox user".to_owned(),
        ));
    }
    let dll = MSYS_DLL
        .iter()
        .fold(tree.clone(), |path, part| path.join(part));
    let runtime = os.runtime(&dll).map_err(Off::Runtime)?;
    let key = msys_key(&runtime.final_path, &dll).ok_or(Off::Moved)?;
    let name = format!(
        "{}-{key}",
        shared_id(&runtime.image).ok_or(Off::NoSharedId)?
    );
    let session = os.session().map_err(Off::ObjectDir)?;
    let sddl = object_dir_sddl(desktop, pool_user);
    for path in object_dirs(session, &name) {
        match os.make_dir(&path, &sddl) {
            Ok(dir) => held.push(dir),
            Err(Made::Exists) => return Err(Off::Squatted),
            Err(Made::Failed(why)) => return Err(Off::ObjectDir(why)),
        }
    }
    Ok(Ready { tree, held })
}

/// The account folder's and the tree id's names when `tree` is `<admin>\git\<account>\<id>`,
/// `<account>` being `pool_name` (without case) and a folder name `git_tree` allows, and
/// `<id>` a tree id's characters (`0-9`, `A-Z`, `a-z`, `.`, `-`), no temporary tree's.
pub(crate) fn tree_parts(
    tree: &WinPath,
    admin: &WinPath,
    pool_name: &str,
) -> Option<(String, String)> {
    let base = admin.join(GIT_DIR).join(pool_name);
    let parts = tree.parts();
    if !folder_name_ok(pool_name) || !tree.within(&base) || parts.len() != base.parts().len() + 1 {
        return None;
    }
    let account = parts.get(parts.len() - 2)?.clone();
    let id = parts.last()?.clone();
    let id_ok = (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !id.starts_with(['.', '-'])
        && !id.ends_with(BUILDING);
    id_ok.then_some((account, id))
}

/// Whether the marker's text is `<id>\n<pool_user>`, a final newline allowed; the id compared
/// without case, as the folder it names is.
pub(crate) fn marker_ok(text: &str, id: &str, pool_user: &Sid) -> bool {
    let mut lines = text.strip_suffix('\n').unwrap_or(text).split('\n');
    let mut line = || lines.next().map(|l| l.strip_suffix('\r').unwrap_or(l));
    line().is_some_and(|found| found.eq_ignore_ascii_case(id))
        && line() == Some(pool_user.as_str())
        && line().is_none()
}

/// msys's key for the runtime whose final path is `final_path` (`\\?\C:\…`), which must be
/// `dll`'s (without case): 16 lowercase hex digits of [`msys_hash`] over its NT form
/// `\??\C:\…`. `None` for another path, a network path or one that isn't ASCII.
pub(crate) fn msys_key(final_path: &str, dll: &WinPath) -> Option<String> {
    let rest = final_path.strip_prefix(r"\\?\")?;
    let found = WinPath::parse(final_path)?;
    if rest.starts_with(r"UNC\")
        || found.drive().is_none()
        || !found.within(dll)
        || !dll.within(&found)
    {
        return None;
    }
    msys_hash(&format!(r"\??\{rest}")).map(|hash| format!("{hash:016x}"))
}

/// msys's `hash_path_name` (`mm/cygheap.cc`) over `text`, from 0: per upper-cased character
/// `c`, `hash = c + (hash << 6) + (hash << 16) - hash`, wrapping. ASCII only, where
/// `RtlUpcaseUnicodeChar` is `to_ascii_uppercase`; `None` otherwise.
pub(crate) fn msys_hash(text: &str) -> Option<u64> {
    text.is_ascii().then(|| {
        text.bytes().fold(0_u64, |hash, byte| {
            u64::from(byte.to_ascii_uppercase())
                .wrapping_add(hash << 6)
                .wrapping_add(hash << 16)
                .wrapping_sub(hash)
        })
    })
}

/// The shared id `image` (`msys-2.0.dll`) carries: what follows its first
/// [`SHARED_ID_TAG`] up to a NUL or a byte that isn't printable, when it is
/// `msys-<digits>.<digits>S<digits>`.
pub(crate) fn shared_id(image: &[u8]) -> Option<String> {
    let at = image
        .windows(SHARED_ID_TAG.len())
        .position(|window| window == SHARED_ID_TAG)?;
    let rest = image.get(at + SHARED_ID_TAG.len()..)?;
    let end = rest
        .iter()
        .position(|byte| !(0x20..0x7f).contains(byte))
        .unwrap_or(rest.len());
    let id = std::str::from_utf8(rest.get(..end)?).ok()?;
    valid_shared_id(id).then(|| id.to_owned())
}

/// Whether `id` is `msys-<digits>.<digits>S<digits>`.
fn valid_shared_id(id: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let Some((major, rest)) = id.strip_prefix("msys-").and_then(|r| r.split_once('.')) else {
        return false;
    };
    let Some((minor, series)) = rest.split_once('S') else {
        return false;
    };
    digits(major) && digits(minor) && digits(series)
}

/// The descriptor of msys's folders: owner the desktop user; SYSTEM and the desktop user
/// `0xF000F` (all directory rights), the pool user `0x2000F` (read control, query, traverse,
/// make objects and folders in it); label Low, no write up.
pub(crate) fn object_dir_sddl(owner: &Sid, pool_user: &Sid) -> String {
    format!(
        "O:{owner}D:(A;;0xF000F;;;SY)(A;;0xF000F;;;{owner})(A;;0x2000F;;;{pool_user})\
         S:(ML;;NW;;;LW)"
    )
}

/// msys's folders named `name` for session `session`: the session's, then the global one;
/// the global one alone in session 0, where they are the same.
pub(crate) fn object_dirs(session: u32, name: &str) -> Vec<String> {
    let global = format!(r"\BaseNamedObjects\{name}");
    if session == 0 {
        vec![global]
    } else {
        vec![
            format!(r"\Sessions\{session}\BaseNamedObjects\{name}"),
            global,
        ]
    }
}

/// `code` as the plain line says it: `[a-z0-9-]`, at most 32, else `unknown`.
fn plain_code(code: &str) -> String {
    let ok = (1..=32).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        code.to_owned()
    } else {
        "unknown".to_owned()
    }
}
