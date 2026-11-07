//! The state folder's clean on Unix (`super::clean`), walked by handle: the agent may swap any
//! folder in its state folder for a link between the clean's look and its removal, so no path
//! below the state folder is resolved by name again. Each folder is opened `O_DIRECTORY |
//! O_NOFOLLOW` through its parent's handle (a link there fails the open and is removed as
//! itself), each entry's kind is read with `fstatat` and `AT_SYMLINK_NOFOLLOW` on the folder's
//! handle, files and links go by `unlinkat` on that handle, and a folder goes by
//! [`remove_dir_at`]: emptied through its own handle, then `unlinkat(.., AT_REMOVEDIR)`.

use std::ffi::{CStr, CString};
use std::io;
use std::path::Path;

use clauseo_protocol::AgentKind;
use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};

use super::{CLAUDE_KEPT_FILES, CLAUDE_LIVE_FOLDERS, CODEX_KEPT_FOLDERS, LOGIN_FILE, codex_keeps};

/// How a folder is opened: for listing only, never through a link.
fn flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

/// An entry's kind, as `fstatat` with `AT_SYMLINK_NOFOLLOW` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A real folder.
    Folder,
    /// A plain file; `true` when it has more than one name.
    File(bool),
    /// A link or anything else.
    Other,
}

/// The names in `dir` (no `.` or `..`).
fn names(dir: &mut Dir) -> Vec<CString> {
    #[cfg(test)]
    hook::count_listing();
    dir.iter()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_owned())
        .filter(|name| name.as_bytes() != b"." && name.as_bytes() != b"..")
        .collect()
}

/// `name`'s kind in `dir`, never through a link; `None` when it is gone.
fn kind(dir: &Dir, name: &CStr) -> io::Result<Option<Kind>> {
    let stat = match fstatat(dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(Errno::ENOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let format = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
    Ok(Some(if format == SFlag::S_IFDIR {
        Kind::Folder
    } else if format == SFlag::S_IFREG {
        Kind::File(stat.st_nlink != 1)
    } else {
        Kind::Other
    }))
}

/// Opens the folder `name` in `dir` by handle; `None` when it is a link or not a folder (the
/// caller removes it as itself) or gone.
fn open_at(dir: &Dir, name: &CStr) -> io::Result<Option<Dir>> {
    match Dir::openat(dir, name, flags(), Mode::empty()) {
        Ok(child) => Ok(Some(child)),
        Err(Errno::ELOOP | Errno::ENOTDIR | Errno::ENOENT) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Removes the name `name` in `dir` itself (a file or a link); a gone one is fine.
fn unlink_at(dir: &Dir, name: &CStr) -> io::Result<()> {
    match unlinkat(dir, name, UnlinkatFlags::NoRemoveDir) {
        Ok(()) | Err(Errno::ENOENT) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The deepest folder [`remove_dir_at`] goes down to; past it the clean refuses, and so the
/// launch fails closed, with the cause in the log.
const MAX_DEPTH: usize = 4096;

/// [`MAX_DEPTH`], lowered by a test.
fn max_depth() -> usize {
    #[cfg(test)]
    {
        hook::max_depth().unwrap_or(MAX_DEPTH)
    }
    #[cfg(not(test))]
    {
        MAX_DEPTH
    }
}

/// Where opening a chain of names stopped.
enum Chain {
    /// Every name opened: the last folder's handle.
    Open(Dir),
    /// The name at this index is a link, not a folder, or gone.
    Broken(usize),
}

/// Opens `names` one after the other from `base`, each through the previous handle with
/// `O_NOFOLLOW`, closing each parent once its child is open: at most two handles at a time.
fn open_chain(base: &Dir, names: &[CString]) -> io::Result<Chain> {
    let mut held: Option<Dir> = None;
    for (at, name) in names.iter().enumerate() {
        let opened = match &held {
            Some(parent) => open_at(parent, name)?,
            None => open_at(base, name)?,
        };
        match opened {
            Some(child) => held = Some(child),
            None => return Ok(Chain::Broken(at)),
        }
    }
    held.map_or(Ok(Chain::Broken(0)), |dir| Ok(Chain::Open(dir)))
}

/// Runs `then` with the handle of `names`' parent chain: `base` itself for an empty one. `None`
/// when that chain no longer opens.
fn with_parent<T>(
    base: &Dir,
    names: &[CString],
    then: impl FnOnce(&Dir) -> io::Result<T>,
) -> io::Result<Option<T>> {
    if names.is_empty() {
        return then(base).map(Some);
    }
    match open_chain(base, names)? {
        Chain::Open(parent) => then(&parent).map(Some),
        Chain::Broken(_) => Ok(None),
    }
}

/// Removes the folder `name` in `base` by handle, deepest first, with a bounded number of open
/// handles at any depth and each folder listed once (linear in its entries, round 6's L1): the
/// chain of names from `base` to the folder being emptied is kept, with each level's child
/// folders still to remove, and re-opened from `base` for each step (each name `O_NOFOLLOW |
/// O_DIRECTORY` through its parent's handle, two handles at a time). In each folder, files and
/// links go by `unlinkat` and the child folders are noted in the one listing; an emptied folder
/// goes by `unlinkat(.., AT_REMOVEDIR)` on its parent's handle (one that is not empty, written
/// to meanwhile, is listed again). A name swapped for a link on the way is removed as itself; no
/// path is ever resolved, so nothing outside is touched. Past [`MAX_DEPTH`] levels the whole
/// folder is set aside (`aside`, the data folder's parking folder): renamed there in one call,
/// logged, and left to the next start's sweep (round 6's D3); with no parking folder it refuses.
pub(super) fn remove_dir_at(base: &Dir, name: &CStr, aside: Option<&Dir>) -> io::Result<()> {
    let mut chain: Vec<CString> = vec![name.to_owned()];
    // For each level of `chain`, its child folders still to remove; `None`: not listed yet.
    let mut pending: Vec<Option<Vec<CString>>> = vec![None];
    while !pending.is_empty() {
        if pending.last().is_some_and(Option::is_none) {
            let mut dir = match open_chain(base, &chain)? {
                Chain::Open(dir) => dir,
                Chain::Broken(at) => {
                    // A link, a file or gone: removed as itself, the walk resumes above it.
                    chain.truncate(at + 1);
                    pending.truncate(at + 1);
                    if let Some((last, parents)) = chain.split_last() {
                        let (last, parents) = (last.clone(), parents.to_vec());
                        with_parent(base, &parents, |parent| unlink_at(parent, &last))?;
                    }
                    chain.pop();
                    pending.pop();
                    continue;
                }
            };
            let mut folders = Vec::new();
            for item in names(&mut dir) {
                match kind(&dir, &item)? {
                    Some(Kind::Folder) => folders.push(item),
                    Some(_) => unlink_at(&dir, &item)?,
                    None => {}
                }
            }
            if let Some(level) = pending.last_mut() {
                *level = Some(folders);
            }
            continue;
        }
        let next = pending
            .last_mut()
            .and_then(Option::as_mut)
            .and_then(Vec::pop);
        if let Some(child) = next {
            if chain.len() >= max_depth() {
                return set_aside_deep(base, name, aside, chain.len());
            }
            chain.push(child);
            pending.push(None);
            continue;
        }
        // Every child of the last level is gone: remove it from its parent.
        let Some((last, parents)) = chain.split_last() else {
            break;
        };
        let (last, parents) = (last.clone(), parents.to_vec());
        let removed = with_parent(base, &parents, |parent| {
            match unlinkat(parent, last.as_c_str(), UnlinkatFlags::RemoveDir) {
                Ok(()) | Err(Errno::ENOENT) => Ok(true),
                // Written to meanwhile: listed again.
                Err(Errno::ENOTEMPTY | Errno::EEXIST) => Ok(false),
                Err(error) => Err(io::Error::from(error)),
            }
        })?;
        if removed == Some(false) {
            if let Some(level) = pending.last_mut() {
                *level = None;
            }
            continue;
        }
        chain.pop();
        pending.pop();
    }
    Ok(())
}

/// Sets aside the folder `name` in `base`, nested past [`MAX_DEPTH`] levels (`levels`): renamed
/// in one call into the parking folder `aside` (`<data>/set-aside`, denied whole to every
/// agent's fence and never granted to any jail), where the next start's run sweep deletes it.
/// Logged with the cause. With no parking folder the clean refuses, and the launch fails closed.
fn set_aside_deep(base: &Dir, name: &CStr, aside: Option<&Dir>, levels: usize) -> io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let Some(aside) = aside else {
        tracing::warn!(
            levels,
            "a folder in a state folder is nested too deep; the clean refuses"
        );
        return Err(io::Error::other("a folder nested too deep to remove"));
    };
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let parked =
        CString::new(format!("deep-{}-{count}", std::process::id())).map_err(io::Error::other)?;
    nix::fcntl::renameat(base, name, aside, parked.as_c_str()).map_err(io::Error::from)?;
    tracing::warn!(
        levels,
        "a folder in a state folder is nested too deep to remove; it is set aside for the next \
         start's sweep"
    );
    Ok(())
}

/// Removes `name` of `kind` in `dir`: a folder by [`remove_dir_at`], anything else as itself.
fn remove_at(dir: &Dir, name: &CStr, kind: Kind, aside: Option<&Dir>) -> io::Result<()> {
    match kind {
        Kind::Folder => remove_dir_at(dir, name, aside),
        Kind::File(_) | Kind::Other => unlink_at(dir, name),
    }
}

/// The clean (`super::clean`) of `state`, by handle. `state` itself is opened with
/// `O_NOFOLLOW`: a state folder swapped for a link fails the clean, and so the launch.
pub(super) fn clean(state: &Path, agent: AgentKind, live: bool) -> io::Result<()> {
    let mut root = Dir::open(state, flags(), Mode::empty()).map_err(io::Error::from)?;
    // The data folder's parking folder, for a tree too deep to remove (`<data>/set-aside`, made
    // by the run sweep before any launch); `None` when it cannot be opened as a real folder.
    let parking = state
        .ancestors()
        .nth(3)
        .map(super::set_aside_dir)
        .and_then(|parking| Dir::open(&parking, flags(), Mode::empty()).ok());
    let aside = parking.as_ref();
    for name in names(&mut root) {
        let Some(found) = kind(&root, &name)? else {
            continue;
        };
        let text = name.to_str().unwrap_or_default();
        let (folder, file) = (found == Kind::Folder, matches!(found, Kind::File(_)));
        let kept = match agent {
            AgentKind::Codex => codex_keeps(text, folder, file),
            AgentKind::Claude if text == "projects" && folder => {
                clean_projects(&root, &name, state, live, aside)?;
                true
            }
            AgentKind::Claude if folder => live && CLAUDE_LIVE_FOLDERS.contains(&text),
            AgentKind::Claude => file && CLAUDE_KEPT_FILES.contains(&text),
        };
        let second = found == Kind::File(true) && text != LOGIN_FILE;
        if kept && second {
            tracing::warn!("a kept state file had a second name; it is removed");
        }
        if !kept || second {
            remove_at(&root, &name, found, aside)?;
        } else if agent == AgentKind::Codex && folder && CODEX_KEPT_FOLDERS.contains(&text) {
            sweep_links(&root, &name, aside)?;
        }
    }
    Ok(())
}

/// In one of Codex's kept folders (`sessions`, `log`, ...), removes, at any depth and by
/// handle, every file with a second name and every link: Codex reads what is there, and a hard
/// or symbolic link could lead it to a credential.
fn sweep_links(dir: &Dir, name: &CStr, aside: Option<&Dir>) -> io::Result<()> {
    sweep_links_at(dir, name, 0, aside)
}

/// How deep [`sweep_links`] goes with a handle per level; a deeper folder (Codex's own are a
/// few levels, `sessions/<year>/<month>/<day>`) is removed whole by [`remove_dir_at`].
const SWEEP_DEPTH: usize = 32;

/// [`sweep_links`] at `depth`.
fn sweep_links_at(dir: &Dir, name: &CStr, depth: usize, aside: Option<&Dir>) -> io::Result<()> {
    if depth >= SWEEP_DEPTH {
        tracing::warn!("a kept Codex folder is nested too deep; its deep part is removed");
        return remove_dir_at(dir, name, aside);
    }
    let Some(mut folder) = open_at(dir, name)? else {
        return unlink_at(dir, name);
    };
    for item in names(&mut folder) {
        match kind(&folder, &item)? {
            Some(Kind::Folder) => sweep_links_at(&folder, &item, depth + 1, aside)?,
            Some(Kind::File(false)) | None => {}
            Some(_) => unlink_at(&folder, &item)?,
        }
    }
    Ok(())
}

/// In Claude's `projects` (`name` in `state`'s handle `root`), keeps each project folder's
/// `*.jsonl` transcripts with one name each and, with `live`, its session folders (never
/// `memory`); everything else goes, by handle.
fn clean_projects(
    root: &Dir,
    name: &CStr,
    state: &Path,
    live: bool,
    aside: Option<&Dir>,
) -> io::Result<()> {
    let Some(mut projects) = open_at(root, name)? else {
        return unlink_at(root, name);
    };
    for project in names(&mut projects) {
        match kind(&projects, &project)? {
            Some(Kind::Folder) => {}
            Some(_) => {
                unlink_at(&projects, &project)?;
                continue;
            }
            None => continue,
        }
        let Some(mut folder) = open_at(&projects, &project)? else {
            unlink_at(&projects, &project)?;
            continue;
        };
        #[cfg(test)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            hook::after_open(
                &state
                    .join("projects")
                    .join(std::ffi::OsStr::from_bytes(project.as_bytes())),
            );
        }
        #[cfg(not(test))]
        let _ = state;
        for item in names(&mut folder) {
            let Some(found) = kind(&folder, &item)? else {
                continue;
            };
            let bytes = item.as_bytes();
            let stays = match found {
                Kind::Folder => live && bytes != b"memory",
                Kind::File(many) => !many && bytes.ends_with(b".jsonl"),
                Kind::Other => false,
            };
            if !stays {
                remove_at(&folder, &item, found, aside)?;
            }
        }
    }
    Ok(())
}

/// Tests only: a hook run after a project folder is opened by handle and before its entries are
/// removed, so a test can swap the folder's name for a link in between.
#[cfg(test)]
pub(super) mod hook {
    use std::cell::RefCell;
    use std::path::Path;

    /// The hook a test set.
    type Hook = Box<dyn Fn(&Path)>;

    thread_local! {
        static AFTER_OPEN: RefCell<Option<Hook>> = RefCell::new(None);
    }

    /// Sets the hook for this thread.
    pub(in crate::agent::state_folder) fn set(hook: impl Fn(&Path) + 'static) {
        AFTER_OPEN.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }

    /// Clears it.
    pub(in crate::agent::state_folder) fn clear() {
        AFTER_OPEN.with(|slot| *slot.borrow_mut() = None);
    }

    thread_local! {
        static MAX_DEPTH: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    }

    /// Lowers the removal's depth cap for this thread (`None`: the real one).
    pub(in crate::agent::state_folder) fn set_max_depth(depth: Option<usize>) {
        MAX_DEPTH.with(|cap| cap.set(depth));
    }

    /// The cap a test set.
    pub(super) fn max_depth() -> Option<usize> {
        MAX_DEPTH.with(std::cell::Cell::get)
    }

    thread_local! {
        static LISTINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Counts a folder listing on this thread.
    pub(super) fn count_listing() {
        LISTINGS.with(|count| count.set(count.get() + 1));
    }

    /// The folder listings on this thread since the last call, and resets the count.
    pub(in crate::agent::state_folder) fn take_listings() -> usize {
        LISTINGS.with(|count| count.replace(0))
    }

    /// Runs it with the project folder's path.
    pub(super) fn after_open(path: &Path) {
        AFTER_OPEN.with(|slot| {
            if let Some(hook) = slot.borrow().as_ref() {
                hook(path);
            }
        });
    }
}
