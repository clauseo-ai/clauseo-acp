//! The PowerShell read allowlist: Claude Code's `PowerShell` lines that only list or read the
//! matter's files or the session's own work and notes folders get the hook's `allow`, with a
//! command the hook writes itself in their place (`updatedInput`).
//!
//! Why: Claude Code allows a read-only PowerShell line itself, before any wildcard rule, and
//! then hands it to Anthropic's server classifier, which refuses many of them on Windows. A hook
//! `allow` skips that classifier, the local one and Claude Code's own path check (measured on
//! 2.1.293: `scratch-S18/hook-premise.md`); only `permissions.deny` still applies, to the
//! rewritten text. So this module does its own path check, and answers only for a line it fully
//! understands; anything else gets no output and Claude Code's normal path runs (fail closed).
//!
//! - The line must match the small grammar in `parse.rs` (Get-ChildItem, Get-Content, `|
//!   Select-Object`, joined by `;`).
//! - The command run is not the agent's text: [`parse::PIN`] (the Management and Utility
//!   modules loaded from `$PSHOME`, or exit 1), then each statement with module-qualified
//!   names, full parameter names and `-LiteralPath '<canonical path>'`. So a lexer difference,
//!   an alias or function shadowing a name, the starting folder, wildcard or number parsing and
//!   quote handling cannot make an allow read anything but the checked paths.
//! - Each path is resolved by the network-safe walk ([`walk`]: no network location touched, each
//!   link read and checked first, on Windows each name in its canonical case and long form). The
//!   result must be local, exist, lie in a qualifying folder ([`Roots`]) and reach no link below
//!   that folder, neither on the way the agent wrote it nor on the canonical path; Get-Content's
//!   target a file with one hard link (another name elsewhere could be outside every folder;
//!   a count that can't be read refuses). A listed folder's entries hold no link; with
//!   `-Recurse` the whole subtree, at most [`MAX_ENTRIES`] entries for the line, hidden ones
//!   included. On Windows a reparse
//!   point other than a link counts only when its tag is a known local kind (a `OneDrive`
//!   placeholder is; `local_reparse_tag`); anything else refuses. A canonical path over 259
//!   characters, or in a `\\?\` form, refuses.
//! - A relative path (and Get-ChildItem with none) is taken from the input's `cwd`, only when
//!   that resolves to the matter folder itself.
//! - The hook answers only with [`FLAG`] in its arguments, which the launch passes for a Claude
//!   session on Windows outside forced ask (`agent/loads.rs`), and only for a tool input whose keys
//!   are `command`, `description`, `timeout` and `run_in_background`.
//!
//! Accepted residual: a link swapped in between the check and PowerShell opening the path is
//! followed (the same window as the data fence's "a link inside a searched folder").

#[cfg(test)]
mod differential_tests;
#[cfg(test)]
mod disk_tests;
mod parse;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::network_paths::resolve::{Disk, Entry, Fs, Walked, walk};
use crate::network_paths::{NetEnv, fold_name, refused_windows_name};
use parse::Source;

/// The argument (after the matter folder) that turns the allowlist on; the folders follow.
pub(crate) const FLAG: &str = "--ps-read";
/// Before a spelling of Clauseo's data folder (repeated).
pub(crate) const DATA_FLAG: &str = "--ps-data";
/// Before the session's work folder.
pub(crate) const WORK_FLAG: &str = "--ps-work";
/// Before the matter's notes folder.
pub(crate) const NOTES_FLAG: &str = "--ps-notes";

/// The most folder entries one line's listings may look at.
pub(crate) const MAX_ENTRIES: usize = 10_000;
/// The longest canonical path written on Windows (`MAX_PATH` less its NUL).
const MAX_WINDOWS_PATH: usize = 259;
/// The tool input keys a line may come with.
const INPUT_KEYS: [&str; 4] = ["command", "description", "timeout", "run_in_background"];
/// What the agent is told with the allow.
pub(crate) const REASON: &str = "Clauseo checked that this PowerShell line only reads the \
     matter's or the session's own files, and runs its own copy of it.";

/// The folders from the arguments: `<data>`'s spellings and the session's own folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Folders {
    data: Vec<PathBuf>,
    work: PathBuf,
    notes: PathBuf,
}

/// The folders after [`FLAG`] in `args`: pairs of [`DATA_FLAG`], [`WORK_FLAG`] or
/// [`NOTES_FLAG`] and an absolute folder, one work, one notes, at least one data. `None`
/// without the flag or for anything else after it.
pub(super) fn folders(args: &[OsString]) -> Option<Folders> {
    let at = args.iter().position(|arg| arg == FLAG)?;
    let (mut data, mut work, mut notes) = (Vec::new(), None, None);
    let pairs = args.get(at.checked_add(1)?..)?;
    if !pairs.len().is_multiple_of(2) {
        return None;
    }
    for [flag, folder] in pairs.as_chunks::<2>().0 {
        let folder = PathBuf::from(folder);
        if !folder.is_absolute() {
            return None;
        }
        let slot = match flag.to_str()? {
            DATA_FLAG => {
                data.push(folder);
                continue;
            }
            WORK_FLAG => &mut work,
            NOTES_FLAG => &mut notes,
            _ => return None,
        };
        if slot.replace(folder).is_some() {
            return None;
        }
    }
    (!data.is_empty()).then_some(Folders {
        data,
        work: work?,
        notes: notes?,
    })
}

/// A folder's entries, each as [`Fs::entry`] would answer, for the listing checks.
pub(crate) trait Tree {
    /// The names in `dir` and what each is (a link as [`Entry::Link`]; an error for a reparse
    /// point of a kind not known to be local), an error past `limit` entries.
    fn children(&self, dir: &Path, limit: usize) -> io::Result<Vec<(OsString, Entry)>>;

    /// How many hard links the file at `file` has, never following a link at it; an error
    /// when that can't be read.
    fn links(&self, file: &Path) -> io::Result<u64>;
}

/// The real disk's listings.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RealTree;

impl Tree for RealTree {
    fn children(&self, dir: &Path, limit: usize) -> io::Result<Vec<(OsString, Entry)>> {
        let mut out = Vec::new();
        for item in std::fs::read_dir(dir)? {
            if out.len() == limit {
                return Err(io::Error::other("too many entries"));
            }
            let item = item?;
            let kind = item.file_type()?;
            let entry = if kind.is_symlink() {
                Entry::Link
            } else if kind.is_dir() {
                Entry::Dir
            } else {
                Entry::File
            };
            #[cfg(windows)]
            let mut entry = entry;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt as _;
                /// `FILE_ATTRIBUTE_REPARSE_POINT`.
                const REPARSE_POINT: u32 = 0x400;
                // From the listing, never opening the entry; only a reparse point is looked at
                // again, by its tag (`RealFs::entry`: a link, a known local kind, or an error).
                let reparse = item.metadata()?.file_attributes() & REPARSE_POINT != 0;
                if entry != Entry::Link && reparse {
                    entry = crate::network_paths::resolve::RealFs.entry(&item.path())?;
                }
            }
            out.push((item.file_name(), entry));
        }
        Ok(out)
    }

    fn links(&self, file: &Path) -> io::Result<u64> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            Ok(std::fs::symlink_metadata(file)?.nlink())
        }
        #[cfg(windows)]
        {
            crate::network_paths::link_count(file).map(u64::from)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = file;
            Err(io::ErrorKind::Unsupported.into())
        }
    }
}

/// The disk looks the allowlist takes.
pub(super) struct Look<'a> {
    pub(super) disk: Disk<'a>,
    pub(super) tree: &'a dyn Tree,
}

/// The hook's stdout for the input `bytes` under `args`, when the allowlist answers it.
pub(super) fn allow(args: &[OsString], bytes: &[u8]) -> Option<String> {
    let folders = folders(args)?;
    let matter = super::matter_arg(args)?;
    let hook: Value = serde_json::from_slice(bytes).ok()?;
    let net = NetEnv::current();
    let look = Look {
        disk: Disk::real(&net),
        tree: &RealTree,
    };
    answer(&hook, &matter, &folders, &look)
}

/// The allow, with its rewritten command, for the hook input `hook` of a session on `matter`
/// with `folders`, looking at the disk through `look`; `None` for anything it does not fully
/// understand and vouch for.
pub(super) fn answer(
    hook: &Value,
    matter: &Path,
    folders: &Folders,
    look: &Look<'_>,
) -> Option<String> {
    if hook.get("tool_name")?.as_str()? != "PowerShell" {
        return None;
    }
    let input = hook.get("tool_input")?.as_object()?;
    if !input.keys().all(|key| INPUT_KEYS.contains(&key.as_str())) {
        return None;
    }
    let line = parse::parse(input.get("command")?.as_str()?, look.disk.net.windows)?;
    let roots = Roots::of(matter, folders, &look.disk)?;
    let cwd = hook.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let mut budget = MAX_ENTRIES;
    let mut paths = Vec::new();
    for statement in &line.statements {
        let target = target(&statement.source, cwd.as_deref(), &roots, look, &mut budget)?;
        paths.push(parse::quoted(target.to_str()?)?);
    }
    let mut updated = input.clone();
    updated.insert(
        "command".to_owned(),
        Value::from(parse::rewrite(&line, &paths)?),
    );
    let answer = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": REASON,
            "updatedInput": updated,
        }
    });
    Some(answer.to_string())
}

/// The folders a line may read, each canonical: the matter folder and the session's own work
/// and notes folders, each only when it qualifies.
#[derive(Debug)]
struct Roots {
    matter: Option<PathBuf>,
    own: Vec<PathBuf>,
}

impl Roots {
    /// The qualifying folders; `None` when none does.
    ///
    /// - The matter: a local folder that is not a drive or file system root, and neither is,
    ///   holds nor lies in any spelling of `<data>`, as given or canonical (names folded, the
    ///   safe side for a refusal).
    /// - The work folder `<data>/run/<id>/work` and notes folder
    ///   `<data>/agent-state/<matter>/notes`: each only while its canonical path is the
    ///   canonical `<data>` with those names added, so a folder swapped for a link counts for
    ///   nothing. Never the run folder itself, `<data>` or another matter's notes.
    fn of(matter: &Path, folders: &Folders, disk: &Disk<'_>) -> Option<Self> {
        if folders.data.is_empty() {
            return None;
        }
        let data: Vec<(&PathBuf, Option<PathBuf>)> = folders
            .data
            .iter()
            .map(|spelling| (spelling, canonical_dir(spelling, disk)))
            .collect();
        let matter = canonical_dir(matter, disk).filter(|canonical| {
            let named = canonical
                .components()
                .any(|part| matches!(part, Component::Normal(_)));
            let clear = data.iter().all(|(spelling, canonical_data)| {
                let spellings = [Some(*spelling), canonical_data.as_ref()];
                spellings
                    .into_iter()
                    .flatten()
                    .all(|data| !overlaps(data, canonical) && !overlaps(data, matter))
            });
            named && clear
        });
        let own: Vec<PathBuf> = [
            (&folders.work, ("run", "work")),
            (&folders.notes, ("agent-state", "notes")),
        ]
        .into_iter()
        .filter_map(|(folder, shape)| own_root(folder, shape, &data, disk))
        .collect();
        (matter.is_some() || !own.is_empty()).then_some(Self { matter, own })
    }

    /// The qualifying folder `target` (canonical) lies in or is.
    fn containing(&self, target: &Path) -> Option<&Path> {
        self.matter
            .iter()
            .chain(&self.own)
            .find(|root| target.starts_with(root))
            .map(PathBuf::as_path)
    }
}

/// `folder`'s canonical path when it is `<data>/<first>/<any name>/<last>` for a spelling in
/// `data` and its canonical path is that spelling's canonical path with the same three names.
fn own_root(
    folder: &Path,
    (first, last): (&str, &str),
    data: &[(&PathBuf, Option<PathBuf>)],
    disk: &Disk<'_>,
) -> Option<PathBuf> {
    data.iter().find_map(|(spelling, canonical_data)| {
        let rel = folder.strip_prefix(spelling).ok()?;
        let names: Vec<&OsStr> = rel
            .components()
            .map(|part| match part {
                Component::Normal(name) => Some(name),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => None,
            })
            .collect::<Option<_>>()?;
        let [head, _, tail] = names.as_slice() else {
            return None;
        };
        if *head != first || *tail != last {
            return None;
        }
        let want = canonical_data.as_ref()?.join(rel);
        (canonical_dir(folder, disk).as_ref() == Some(&want)).then_some(want)
    })
}

/// Where the absolute `path` leads, when that is a local folder that exists.
fn canonical_dir(path: &Path, disk: &Disk<'_>) -> Option<PathBuf> {
    match walk(path, disk) {
        Walked::Local(canonical) if disk.fs.entry(&canonical).ok() == Some(Entry::Dir) => {
            Some(canonical)
        }
        Walked::Local(_) | Walked::Refused => None,
    }
}

/// Whether `a` is `b`, holds it or lies in it, names compared folded ([`fold_name`]: it
/// over-matches, the safe side for this refusal).
fn overlaps(a: &Path, b: &Path) -> bool {
    let prefix = |short: &Path, long: &Path| {
        let mut names = long.components();
        short
            .components()
            .all(|want| names.next().is_some_and(|got| folded(got) == folded(want)))
    };
    prefix(a, b) || prefix(b, a)
}

fn folded(part: Component<'_>) -> String {
    fold_name(&part.as_os_str().to_string_lossy())
}

/// The canonical path `source` reads, once every check passed (module doc), with `budget` the
/// listing entries left for the line.
fn target(
    source: &Source,
    cwd: Option<&Path>,
    roots: &Roots,
    look: &Look<'_>,
    budget: &mut usize,
) -> Option<PathBuf> {
    let net = look.disk.net;
    let written = match source.path() {
        Some(path) if path.absolute => PathBuf::from(&path.text),
        relative => {
            let base = canonical_dir(cwd?, &look.disk)?;
            if roots.matter.as_ref() != Some(&base) {
                return None;
            }
            match relative {
                Some(path) => base.join(&path.text),
                None => base,
            }
        }
    };
    if refused_windows_name(&written, net) {
        return None;
    }
    let recorder = Recorder {
        fs: look.disk.fs,
        links: RefCell::new(Vec::new()),
    };
    let disk = Disk {
        fs: &recorder,
        real: look.disk.real,
        net,
    };
    let Walked::Local(target) = walk(&written, &disk) else {
        return None;
    };
    let root = roots.containing(&target)?;
    let links = recorder.links.borrow();
    if links
        .iter()
        .any(|link| link.parent().is_some_and(|parent| parent.starts_with(root)))
    {
        return None;
    }
    if net.windows {
        let text = target.to_str()?;
        if text.starts_with('\\') || text.chars().count() > MAX_WINDOWS_PATH {
            return None;
        }
    }
    // Every name below the root, on the canonical path itself, is a folder; the target what the
    // cmdlet reads.
    let below_ok = target == root
        || target
            .ancestors()
            .skip(1)
            .take_while(|folder| *folder != root)
            .all(|folder| look.disk.fs.entry(folder).ok() == Some(Entry::Dir));
    if !below_ok {
        return None;
    }
    match (source, look.disk.fs.entry(&target).ok()?) {
        // A second name elsewhere (a hard link) would let the read reach a file outside the
        // folder; the count can't be read: refuse.
        (Source::Read { .. }, Entry::File) => {
            if look.tree.links(&target).ok()? != 1 {
                return None;
            }
        }
        (Source::List { .. }, Entry::File) => {}
        (Source::List { recurse, .. }, Entry::Dir) => {
            if !clean(&target, *recurse, look.tree, budget) {
                return None;
            }
        }
        _ => return None,
    }
    Some(target)
}

/// Whether the folder `dir`'s entries (with `recurse`, its whole subtree's) hold no link and
/// nothing [`Tree::children`] refuses, within `budget` entries (taken from it).
fn clean(dir: &Path, recurse: bool, tree: &dyn Tree, budget: &mut usize) -> bool {
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(children) = tree.children(&folder, *budget) else {
            return false;
        };
        for (name, entry) in children {
            let Some(left) = budget.checked_sub(1) else {
                return false;
            };
            *budget = left;
            match entry {
                Entry::Link => return false,
                Entry::Dir if recurse => folders.push(folder.join(name)),
                Entry::Dir | Entry::File => {}
            }
        }
    }
    true
}

/// An [`Fs`] that notes each link the walk meets (the path of the link itself).
struct Recorder<'a> {
    fs: &'a dyn Fs,
    links: RefCell<Vec<PathBuf>>,
}

impl Fs for Recorder<'_> {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        let entry = self.fs.entry(path)?;
        if entry == Entry::Link {
            self.links.borrow_mut().push(path.to_path_buf());
        }
        Ok(entry)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fs.read_link(path)
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        self.fs.identity(path)
    }
}
