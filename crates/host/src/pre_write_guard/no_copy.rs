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
//! **Without the list** the rule still looks at each target itself ([`live`]): a file over
//! [`FILE_CAP_BYTES`], or one the OS marks cloud-only ([`OsProbe`], a stat, never a download),
//! is refused too. So a missing or planted list never lets a delete of such a file through that
//! the guard can see; it only loses the files hydrated earlier in the turn, which the list still
//! names.
//!
//! **What it refuses** (after the held-open and `.codex` rules):
//!
//! - a file tool on a listed file: Claude Code's `Write`, `Edit`, `MultiEdit`, `NotebookEdit`;
//!   any file of a Codex `apply_patch` (`Add`, `Update`, `Delete`, a move's both sides), and a
//!   patch inside a shell command;
//! - a shell command (`Bash`, `PowerShell`) that writes, deletes or moves a listed file as the
//!   turn's own shell writes are read (`crate::shell_writes`, one reader, so the end of the turn
//!   credits the same forms): `>` and `>>`, `-o`/`--output`, `tee`, `cp`'s last argument, `rm`,
//!   `unlink`, `mv` (both sides), `Remove-Item`, `del`, `erase`, `ri`, `rd`, `Move-Item`, what a
//!   `stetdoc` command writes, after any `cd` it can follow;
//! - a delete program (not a move) naming a folder that holds a listed file (`rm -r Exhibits`,
//!   `rm -r Exhibits/`), the matter folder itself or one above it (`rm -rf .`, `rm -rf ..`).
//!
//! Not caught, left to the end of the turn: a glob or an expansion (`rm *.pdf`, `rm -rf *`,
//! `rm "$f"`), `find -delete`, `xargs rm`, `sed -i`, a script file or an
//! inline script (`python -c`), `rsync --delete`, `unzip -o`, a file that became no-copy during
//! the turn by a route the live check does not see, and a file made and deleted in one turn.
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

use super::Refusal;
use super::codex::patch_targets;
use super::names::disk_key;
use super::reach::Reach;
use super::shell::walked_resolutions;
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

/// What the agent is told when it may not delete or overwrite `path`.
pub(super) fn reason(path: &MatterPath, why: SkippedFileReason) -> String {
    let because = match why {
        SkippedFileReason::OverSizeCap => "it is larger than 512 MB",
        SkippedFileReason::CloudOnly
        | SkippedFileReason::UnreadFolder
        | SkippedFileReason::Unknown => {
            "it is only online, a cloud file not downloaded to this computer"
        }
    };
    format!(
        "{path} can't be brought back if it is deleted, moved or overwritten: Clauseo keeps no \
         copy of it, because {because}. Don't delete, move or overwrite it. Ask the lawyer \
         first, or write to a new file."
    )
}

/// The no-copy refusal of the hook input `bytes`, if any, with `matter` the folder from the
/// arguments and `list` the session's list. Input the guard cannot read is no refusal here: the
/// other rules already let it go (`Ignored`).
pub(super) fn refusal(bytes: &[u8], matter: Option<&Path>, list: &List) -> Option<Refusal> {
    let hook: Value = serde_json::from_slice(bytes).ok()?;
    let cwd = hook
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|cwd| cwd.is_absolute());
    let folder = matter.map(Path::to_path_buf).or_else(|| cwd.clone())?;
    let base = cwd.unwrap_or_else(|| folder.clone());
    let tool = hook.get("tool_name")?.as_str()?;
    let (writes, deletes) = targets(tool, hook.get("tool_input")?, &base)?;
    let reach = Reach::new(&folder);
    let check = |paths: &[PathBuf], delete: bool| {
        paths
            .iter()
            .flat_map(|path| walked_resolutions(&base.join(path), &reach))
            .find_map(|resolved| at(list, &folder, &resolved, delete))
    };
    check(&writes, false).or_else(|| check(&deletes, true))
}

/// The paths the tool `tool` with `input` writes, deletes or moves, and those a delete program
/// names (for the folder rule), relative ones taken from `base`; `None` for a tool the guard
/// does not check.
fn targets(tool: &str, input: &Value, base: &Path) -> Option<(Vec<PathBuf>, Vec<PathBuf>)> {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let dialect = match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => {
            let paths = match text("file_path").or_else(|| text("notebook_path")) {
                Some(path) => vec![PathBuf::from(path)],
                None => patch_targets(text("command")?),
            };
            return Some((paths, Vec::new()));
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
    let mut paths: Vec<PathBuf> = writes.into_iter().map(PathBuf::from).collect();
    paths.extend(text("command").map(patch_targets).unwrap_or_default());
    Some((paths, deletes.into_iter().map(PathBuf::from).collect()))
}

/// The refusal for a write to `resolved` (walked, absolute) in the matter `folder`: a listed
/// file, under `delete` a folder holding one, else a file [`live`] finds has no copy.
fn at(list: &List, folder: &Path, resolved: &Path, delete: bool) -> Option<Refusal> {
    if delete && is_folder(folder, resolved) {
        let (path, why) = list.first()?;
        return Some(Refusal::NoCopy(path, why));
    }
    let inside = clauseo_matters::matter_path_of_resolved(folder, resolved)?;
    let listed = list
        .file(&inside)
        .or_else(|| delete.then(|| list.under(&inside)).flatten());
    if let Some((path, why)) = listed {
        return Some(Refusal::NoCopy(path, why));
    }
    live(resolved).map(|why| Refusal::NoCopy(inside, why))
}

/// Whether `resolved` (walked) is the matter `folder` itself (`rm -rf .`), or a folder above it.
fn is_folder(folder: &Path, resolved: &Path) -> bool {
    let canonical = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    folder.starts_with(resolved) || canonical.starts_with(resolved)
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
