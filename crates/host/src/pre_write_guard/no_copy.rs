//! The no-copy rule: the guard refuses an agent's delete or overwrite of a file Clauseo holds no
//! copy of, a cloud-only placeholder or a file over the size cap, which Versions skipped at the
//! turn's start (`clauseo_matters::Checkpoint::no_copy`), since no undo could bring it back. The
//! end of the turn still names any such file that went (`actor/turn/end/files.rs`): this rule
//! stops the forms the guard can read before they run, that one catches the rest.
//!
//! **The list.** With [`FLAG`] and a file, and [`ID_FLAG`] and that file's identity (both agents'
//! hooks pass `<data>/browser/guard/<session>/no-copy.txt`, beside the guard's record), the guard
//! reads the files from that list. The host makes it at the launch, holding only its header, with
//! `O_EXCL`, in the session's guard folder, which the host makes create-new and no agent's shell or
//! patch tool may write ([`make`], `launch_checks/folders.rs` `make_guard`), and puts the identity
//! of the file it made (`write::RecordId`, the record's: device and inode, or the volume serial and
//! file ID on Windows) in the hook command beside its path. The file is never replaced, since a new
//! file is a new identity: after each before-turn checkpoint the host appends the changes to the
//! checkpoint's list, an `add` or a `remove` line each (`matter_worker/no_copy_list.rs`). The guard
//! only reads it, and only the regular file with one link whose opened handle has that identity,
//! opened through no link at its last part and never waiting on a pipe (`write::open_read_record`);
//! a link, a reparse point, a pipe or a folder there, a second link to it, another file, a file it
//! cannot read, or one without its header reads as an empty list. A list longer than
//! [`MAX_BYTES`] is read up to its last whole line within that, and the host never appends past
//! it. It folds the lines in order, a line it cannot read skipped. Text:
//!
//! ```text
//! clauseo-no-copy 1
//! add cloud-only Drafts/Scan.pdf
//! add over-size-cap Exhibits/Video.mp4
//! remove Drafts/Scan.pdf
//! ```
//!
//! **What is refused** (after the held-open and `.codex` rules). A lawyer can rely on these:
//!
//! - a **delete** of a listed file, or of a file the guard sees is cloud-only or over
//!   [`FILE_CAP_BYTES`] now ([`live`]: a stat, never a download), whether or not it is on the
//!   disk now: Codex's `apply_patch` `Delete File` (and a patch inside a shell command); a
//!   single, literal `rm`, `unlink`, `Remove-Item`, `del`, `erase`, `ri` or `rd` naming it,
//!   after any `cd` the shell reader can follow;
//! - a delete program naming a folder that holds a listed file (`rm -r Exhibits`,
//!   `rm -r Exhibits/`), or the matter folder itself or one above it while anything is listed
//!   (`rm -rf .`, `rm -rf ..`);
//! - an **overwrite, edit or move** of a file that is cloud-only or over the cap *now*
//!   ([`live`]): Claude Code's `Write`, `Edit`, `MultiEdit`, `NotebookEdit`; a Codex patch's
//!   `Add`, `Update` and `Move to`; and the shell forms the shared write-form list reads
//!   (`crate::shell_writes`, the one reader the end of the turn credits by): `>` and `>>`,
//!   `-o`/`--output`, `tee`, `cp`'s last argument, `mv` and `Move-Item` (both sides), what a
//!   `stetdoc` command writes. The shell forms grow with that list; nothing here adds its own.
//!
//! **What is not refused**, left to the end of the turn (`actor/turn/end/files.rs`, which names
//! a deleted no-copy file, and one changed in place, after the turn):
//!
//! - an overwrite or edit of a listed file that is on the disk now and within the cap (the
//!   agent's read of an online-only file downloads it; refusing its edit for the rest of the turn
//!   would block ordinary work): the end of the turn names it as changed with no earlier copy;
//! - a shell form the shared write-form list does not read. That list (`crate::shell_writes`)
//!   is the one place the read forms are kept; this rule refuses those forms and no others, and
//!   the end of the turn names what went by any other;
//! - a file that became no-copy during the turn by a route the live check does not see, and a
//!   file made and deleted in one turn;
//! - on a matter kept on a network share, nothing is looked at on the disk: a delete of a listed
//!   file is still refused (the list is looked up by the path's text), but the live check, and so
//!   every overwrite refusal, does not apply there.
//!
//! **Open until the shared write-form list tells a move's source from its destination**: moving
//! or renaming a listed file that is still cloud-only or over the cap is refused as an overwrite
//! (a move loses nothing, so it should not be); and the end of the turn's credit of a shell
//! command's folder targets (`clauseo_matters` `diff.rs` `removed`) takes every write target as a
//! folder prefix, not only delete and move arguments, so a lawyer's own delete under a folder a
//! `cp` named can be credited to the agent.
//!
//! **Without a list, or with one the guard does not trust**, a delete is judged by the live check
//! alone: it still refuses a file that is cloud-only or over the cap now, but no longer a listed
//! file downloaded earlier in the turn, nor a folder holding one (`rm -r Exhibits`, `rm -rf .`).
//! On Windows the list file has the protection of Clauseo's other data-folder files there (the
//! command-text rules; there is no fence), so on Windows the live check is the part to rely on.
//!
//! A refusal names the file as the list spells it, and it is recorded for the lawyer like the
//! held-open one (`record.rs` `Reason::NoCopy`).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};

use clauseo_matters::{FILE_CAP_BYTES, OsProbe, PlaceholderProbe};
use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};
use serde_json::Value;

use super::codex::{patch_deletes, patch_targets};
use super::names::disk_key;
use super::reach::Reach;
use super::shell::walked_resolutions;
use super::{Refusal, lexical};
use crate::shell_writes::{self, Dialect};
use crate::write::RecordId;

/// The list's name, in the session's guard folder (`record::folder`).
pub(crate) const FILE_NAME: &str = "no-copy.txt";

/// The argument that names the list, followed by the file.
pub(crate) const FLAG: &str = "--no-copy-list";

/// The argument that names the list's identity ([`RecordId::hex`]), followed by it.
pub(crate) const ID_FLAG: &str = "--no-copy-list-id";

/// A session's list: the file and the identity the host made it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoCopyList {
    /// `<data>/browser/guard/<session>/no-copy.txt`.
    pub(crate) file: PathBuf,
    /// The identity of the file made.
    pub(crate) id: RecordId,
}

impl NoCopyList {
    /// The file and its identity as the hooks pass them, after [`FLAG`] and [`ID_FLAG`].
    pub(crate) fn hook_arg(&self) -> (&Path, String) {
        (&self.file, self.id.hex())
    }
}

/// Makes the list in the session's guard folder `folder` (made create-new just before,
/// `launch_checks/folders.rs` `make_guard`): the file with `O_EXCL` (`write::create_record`),
/// then its header appended through the identity it was made with. Anything already there fails
/// with `AlreadyExists`, never followed. File system calls: the launch prep's, off the actor.
pub(crate) fn make(folder: &Path) -> std::io::Result<NoCopyList> {
    let file = folder.join(FILE_NAME);
    let id = crate::write::create_record(&file)?;
    crate::write::open_append_record(&file, id)?.write_all(HEADER.as_bytes())?;
    Ok(NoCopyList { file, id })
}

/// The list's first line.
pub(crate) const HEADER: &str = "clauseo-no-copy 1\n";

/// The most files the host lists; the rest go unlisted (the live check still applies).
pub(crate) const MAX_FILES: usize = 20_000;

/// The most of a list the guard reads, and the most the host writes
/// (`matter_worker/no_copy_list.rs` stops appending there): a longer list is read up to its last
/// whole line within this, never as empty.
pub(crate) const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The files a list names, each by its key as this system's file system compares names.
#[derive(Debug, Default)]
pub(crate) struct List {
    files: HashMap<String, (MatterPath, SkippedFileReason)>,
    /// Every folder above a listed file, by key.
    folders: HashSet<String>,
}

impl List {
    /// The list in the file after [`FLAG`] in `args`, pinned by the identity after
    /// [`ID_FLAG`]; empty without both, and for anything [`Self::read`] does not trust.
    pub(super) fn of_args(args: &[OsString]) -> Self {
        match (arg(args, FLAG), arg(args, ID_FLAG)) {
            (Some(file), Some(id)) if Path::new(&file).is_absolute() => {
                match id.to_str().and_then(RecordId::parse) {
                    Some(id) => Self::read(&NoCopyList {
                        file: PathBuf::from(file),
                        id,
                    }),
                    None => Self::default(),
                }
            }
            _ => Self::default(),
        }
    }

    /// The list `list` names, empty unless it is a readable regular file with one link, with
    /// the identity the host made it with.
    pub(crate) fn read(list: &NoCopyList) -> Self {
        let Ok(opened) = crate::write::open_read_record(&list.file, list.id) else {
            return Self::default();
        };
        let mut bytes = Vec::new();
        if opened
            .take(MAX_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Self::default();
        }
        Self::decode(&String::from_utf8_lossy(within_limit(&bytes)))
    }

    /// The list `text` holds, its lines folded in order; empty without the header, a line it
    /// cannot read skipped.
    pub(crate) fn decode(text: &str) -> Self {
        let Some(lines) = text.strip_prefix(HEADER) else {
            return Self::default();
        };
        let mut files: HashMap<String, (MatterPath, SkippedFileReason)> = HashMap::new();
        for line in lines.lines() {
            if let Some(rest) = line.strip_prefix("remove ") {
                if let Ok(path) = MatterPath::new(rest) {
                    files.remove(&disk_key(path.as_str()));
                }
            } else if let Some(file) = line.strip_prefix("add ").and_then(added) {
                files.insert(disk_key(file.path.as_str()), (file.path, file.reason));
            }
        }
        let mut folders = HashSet::new();
        let mut keys: Vec<&String> = files.keys().collect();
        keys.sort_unstable();
        for key in keys {
            let mut above = key.as_str();
            while let Some((folder, _)) = above.rsplit_once('/') {
                folders.insert(folder.to_owned());
                above = folder;
            }
        }
        Self { files, folders }
    }

    /// The lines that make this list `files` (cloud-only or over the cap, the first
    /// [`MAX_FILES`]): a `remove` for each listed file not in it, then an `add` for each file
    /// not listed so, sorted; empty when nothing changes.
    pub(crate) fn changes(&self, files: &[SkippedFile]) -> String {
        let wanted: HashMap<String, &SkippedFile> = files
            .iter()
            .filter(|file| spelled(file.reason).is_some())
            .take(MAX_FILES)
            .map(|file| (disk_key(file.path.as_str()), file))
            .collect();
        let mut removed: Vec<&str> = self
            .files
            .iter()
            .filter(|(key, _)| !wanted.contains_key(*key))
            .map(|(_, (path, _))| path.as_str())
            .collect();
        removed.sort_unstable();
        let mut added: Vec<&SkippedFile> = wanted
            .iter()
            .filter(|(key, file)| {
                self.files
                    .get(*key)
                    .is_none_or(|(path, reason)| *path != file.path || *reason != file.reason)
            })
            .map(|(_, file)| *file)
            .collect();
        added.sort_unstable_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        let mut text = String::new();
        for path in removed {
            for part in ["remove ", path, "\n"] {
                text.push_str(part);
            }
        }
        for file in added {
            let reason = spelled(file.reason).unwrap_or_default();
            for part in ["add ", reason, " ", file.path.as_str(), "\n"] {
                text.push_str(part);
            }
        }
        text
    }

    /// The listed file `path` names, as the list spells it.
    fn file(&self, path: &MatterPath) -> Option<(MatterPath, SkippedFileReason)> {
        self.files.get(&disk_key(path.as_str())).cloned()
    }

    /// Whether `path` is listed.
    #[cfg(test)]
    pub(crate) fn names(&self, path: &MatterPath) -> bool {
        self.file(path).is_some()
    }

    /// The first listed file (by path bytes), when one is.
    fn first(&self) -> Option<(MatterPath, SkippedFileReason)> {
        self.files
            .values()
            .min_by(|a, b| a.0.as_str().cmp(b.0.as_str()))
            .cloned()
    }

    /// The first listed file (by path bytes) under the folder `path`, when one is.
    fn under(&self, path: &MatterPath) -> Option<(MatterPath, SkippedFileReason)> {
        let key = disk_key(path.as_str());
        if !self.folders.contains(&key) {
            return None;
        }
        let prefix = format!("{key}/");
        self.files
            .iter()
            .filter(|(listed, _)| listed.starts_with(&prefix))
            .map(|(_, file)| file.clone())
            .min_by(|a, b| a.0.as_str().cmp(b.0.as_str()))
    }
}

/// `bytes`, or when longer than [`MAX_BYTES`] its whole lines within that: a line cut at the
/// limit is left out, the lines before it kept.
fn within_limit(bytes: &[u8]) -> &[u8] {
    let Ok(limit) = usize::try_from(MAX_BYTES) else {
        return bytes;
    };
    if bytes.len() <= limit {
        return bytes;
    }
    let head = bytes.get(..limit).unwrap_or(bytes);
    head.iter()
        .rposition(|byte| *byte == b'\n')
        .and_then(|at| head.get(..=at))
        .unwrap_or_default()
}

/// The first lines of `changes` that fit after a list of `len` bytes within [`MAX_BYTES`]:
/// the host never appends past what the guard reads. `remove` lines come first in
/// [`List::changes`], so they are the last to be left out.
pub(crate) fn fitting(changes: &str, len: u64) -> &str {
    let room = usize::try_from(MAX_BYTES.saturating_sub(len)).unwrap_or(usize::MAX);
    if changes.len() <= room {
        return changes;
    }
    let head = changes.get(..room).unwrap_or_default();
    head.rfind('\n')
        .map_or("", |at| changes.get(..=at).unwrap_or_default())
}

/// How a list line spells `reason`; `None` for one that is never listed.
fn spelled(reason: SkippedFileReason) -> Option<&'static str> {
    match reason {
        SkippedFileReason::CloudOnly => Some("cloud-only"),
        SkippedFileReason::OverSizeCap => Some("over-size-cap"),
        SkippedFileReason::UnreadFolder | SkippedFileReason::Unknown => None,
    }
}

/// The file of an `add` line's `rest`: `<reason> <path>`.
fn added(rest: &str) -> Option<SkippedFile> {
    let (reason, path) = rest.split_once(' ')?;
    let reason = match reason {
        "cloud-only" => SkippedFileReason::CloudOnly,
        "over-size-cap" => SkippedFileReason::OverSizeCap,
        _ => return None,
    };
    Some(SkippedFile {
        path: MatterPath::new(path).ok()?,
        reason,
    })
}

/// The argument after `flag` in `args`, if any.
fn arg(args: &[OsString], flag: &str) -> Option<OsString> {
    let at = args.iter().position(|arg| arg == flag)?;
    args.get(at.saturating_add(1)).cloned()
}

/// What a refused call would have done to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// Deleted it (`rm`, `Remove-Item`, a patch's `Delete File`, a folder holding it).
    Delete,
    /// Overwritten, edited or moved it, while it is cloud-only or over the size cap.
    Overwrite,
}

/// What the agent is told when it may not delete or overwrite `path`, for `why`: true in each
/// case (a listed file being deleted may be on the disk now, so a cloud file is "kept only
/// online", never "not downloaded"), and never a promise that asking unlocks it.
pub(super) fn reason(path: &MatterPath, why: SkippedFileReason, act: Act) -> String {
    let because = match why {
        SkippedFileReason::OverSizeCap => "it is larger than 512 MB",
        SkippedFileReason::CloudOnly
        | SkippedFileReason::UnreadFolder
        | SkippedFileReason::Unknown => match act {
            Act::Delete => "it was only online, not on this computer, when this answer began",
            Act::Overwrite => "it is only online, a cloud file not downloaded to this computer",
        },
    };
    match act {
        Act::Delete => format!(
            "{path} can't be brought back if it is deleted: Clauseo keeps no copy of it, because \
             {because}. Clauseo does not let you delete it. Tell the lawyer; they can delete it \
             themselves."
        ),
        Act::Overwrite => format!(
            "{path} can't be brought back if it is overwritten: Clauseo keeps no copy of it, \
             because {because}. Write to a new file instead, and tell the lawyer."
        ),
    }
}

/// The no-copy refusal of the hook input `bytes`, if any, with `matter` the folder from the
/// arguments and `list` reading the session's list, called only when the call deletes something
/// (an overwrite is judged by the file's state now alone, [`live`]). Input the guard cannot read
/// is no refusal here: the other rules already let it go (`Ignored`).
pub(super) fn refusal(
    bytes: &[u8],
    matter: Option<&Path>,
    list: impl FnOnce() -> List,
) -> Option<Refusal> {
    let hook: Value = serde_json::from_slice(bytes).ok()?;
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute());
    let folder = matter.map(Path::to_path_buf).or_else(|| cwd.clone())?;
    let base = cwd.unwrap_or_else(|| folder.clone());
    let tool = hook.get("tool_name")?.as_str()?;
    let Targets { writes, deletes } = targets(tool, hook.get("tool_input")?, &base)?;
    let reach = Reach::new(&folder);
    let overwrite = writes
        .iter()
        .flat_map(|path| walked_resolutions(&base.join(path), &reach))
        .find_map(|resolved| overwritten(&folder, &resolved));
    if overwrite.is_some() || deletes.is_empty() {
        return overwrite;
    }
    let list = list();
    deletes.iter().find_map(|path| {
        let path = base.join(path);
        let walked = walked_resolutions(&path, &reach);
        if walked.is_empty() {
            // A path the guard may not touch (a matter kept on a network share): the list is
            // looked up by its text alone, and nothing is looked at on the disk.
            deleted(&list, &folder, &lexical(&path), false)
        } else {
            walked
                .iter()
                .find_map(|resolved| deleted(&list, &folder, resolved, true))
        }
    })
}

/// The paths a call overwrites or moves (`writes`) and those it deletes (`deletes`), relative
/// ones taken from its folder.
#[derive(Debug, Default)]
struct Targets {
    writes: Vec<PathBuf>,
    deletes: Vec<PathBuf>,
}

/// The paths the tool `tool` with `input` writes or moves, and those it deletes, relative ones
/// taken from `base`; `None` for a tool the guard does not check. A shell's delete is what a
/// delete program names (`shell_writes::add_deleted_input`), a patch's its `Delete File`; every
/// other target, a move's both sides included, is a write.
fn targets(tool: &str, input: &Value, base: &Path) -> Option<Targets> {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let dialect = match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
            return Some(match text("file_path").or_else(|| text("notebook_path")) {
                Some(path) => Targets {
                    writes: vec![PathBuf::from(path)],
                    deletes: Vec::new(),
                },
                None => patch_split(text("command")?),
            });
        }
        "Bash" => Dialect::Posix,
        "PowerShell" => Dialect::PowerShell,
        _ => return None,
    };
    let raw = input.to_string();
    let start = base.to_str();
    let (mut writes, mut deletes) = (BTreeSet::new(), BTreeSet::new());
    shell_writes::add_input(&mut writes, &raw, dialect, start);
    shell_writes::add_deleted_input(&mut deletes, &raw, dialect, start);
    let patch = text("command").map(patch_split).unwrap_or_default();
    let mut found = Targets {
        writes: writes
            .difference(&deletes)
            .map(PathBuf::from)
            .chain(patch.writes)
            .collect(),
        deletes: deletes.into_iter().map(PathBuf::from).collect(),
    };
    found.deletes.extend(patch.deletes);
    Some(found)
}

/// A patch's targets: its `Delete File` lines deleted, every other file written.
fn patch_split(patch: &str) -> Targets {
    let deletes = patch_deletes(patch);
    let writes = patch_targets(patch)
        .into_iter()
        .filter(|path| !deletes.contains(path))
        .collect();
    Targets { writes, deletes }
}

/// The refusal for an overwrite or a move of `resolved` (walked, absolute) in the matter
/// `folder`: only a file that is cloud-only now, or over the size cap ([`live`]). A listed file
/// that is on the disk now and within the cap may be overwritten: the agent's read downloaded it,
/// and refusing every edit of it for the rest of the turn would block ordinary work.
fn overwritten(folder: &Path, resolved: &Path) -> Option<Refusal> {
    let inside = clauseo_matters::matter_path_of_resolved(folder, resolved)?;
    live(resolved).map(|why| Refusal::NoCopy(inside, why, Act::Overwrite))
}

/// The refusal for a delete of `resolved` (absolute; walked when `touched`, else by its text
/// alone) in the matter `folder`: the matter folder or one above it when anything is listed, a
/// listed file, a folder holding one, else (when `touched`) a file [`live`] finds has no copy.
fn deleted(list: &List, folder: &Path, resolved: &Path, touched: bool) -> Option<Refusal> {
    let refused = |(path, why)| Refusal::NoCopy(path, why, Act::Delete);
    if is_folder(folder, resolved, touched) {
        return list.first().map(refused);
    }
    let inside = if touched {
        clauseo_matters::matter_path_of_resolved(folder, resolved)?
    } else {
        MatterPath::from_relative(resolved.strip_prefix(lexical(folder)).ok()?)?
    };
    if let Some(listed) = list.file(&inside).or_else(|| list.under(&inside)) {
        return Some(refused(listed));
    }
    touched
        .then(|| live(resolved))
        .flatten()
        .map(|why| refused((inside, why)))
}

/// Whether `resolved` is the matter `folder` itself (`rm -rf .`), or a folder above it; the
/// folder's real path is looked up only when `touched` (the guard may look at the disk).
fn is_folder(folder: &Path, resolved: &Path, touched: bool) -> bool {
    folder.starts_with(resolved)
        || (touched
            && dunce::canonicalize(folder).is_ok_and(|canonical| canonical.starts_with(resolved)))
}

/// Why Clauseo could hold no copy of the file at `resolved`, by looking at it now: over
/// [`FILE_CAP_BYTES`], or cloud-only ([`OsProbe`], its flags; nothing is read or downloaded).
/// `None` for anything but a regular file, reached through no link at its last part.
pub(super) fn live(resolved: &Path) -> Option<SkippedFileReason> {
    let metadata = std::fs::symlink_metadata(resolved).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.len() > FILE_CAP_BYTES {
        Some(SkippedFileReason::OverSizeCap)
    } else if OsProbe.is_cloud_only(resolved, &metadata) {
        Some(SkippedFileReason::CloudOnly)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
