//! Where stetdoc's reads may go. Claude's `Bash(stetdoc <sub>:*)` allow rules
//! (`clauseo_profile::claude::stetdoc_allowed`) run `view`, `inspect`, `check` and `lookup` with
//! no question, and Claude Code checks no path for stetdoc, so its `Read` and shell deny rules
//! never see the document a command names. stetdoc follows a link given as its input and reads
//! any Word document or working copy the lawyer can read. So the guard fences it ([`GuardFence`]):
//! the data folder (but the session's own folders in it), another matter's folder, a protected
//! root. A template in the lawyer's Documents is ordinary use and reads as before.
//!
//! The fence does not try to read every shell line. It allows one shape and refuses any other
//! line that mentions stetdoc at all ([`super::shape`]): plain simple commands joined by `;`, `&&`,
//! `||`, `|` or a new line, every word a plain literal, no wildcard, variable, substitution,
//! redirection (but `2>&1`), here-document or PowerShell splat. In that line:
//!
//! - a command that runs stetdoc names it first, as `stetdoc`, and is the first of its pipe;
//! - every other command is a filter from a short list ([`FILTERS`]) with no word mentioning
//!   stetdoc, so no folder change, alias, function, variable, link, wrapper or other shell;
//! - each word of a stetdoc command that is not an option name, and each option's `=value`, is
//!   walked to its real path ([`walk`]: every link followed, each name's long form, so an 8.3
//!   short name too) and refused in a fenced place, for every subcommand;
//! - a reading subcommand's document must exist and have one name (no hard link).
//!
//! A word the walk can't vouch for (a network path, a link it can't read) is refused. Git Bash's
//! `/c/…`, `/cygdrive/c/…` and `/proc/cygdrive/c/…` are read as Git Bash hands them on; any
//! other path from its root (`/tmp`, `/usr`) is refused on Windows.
//!
//! Not caught here: a link swapped between this check and stetdoc opening the path (a loop run
//! in the background by an earlier command), and a hard link inside a working copy that
//! `check` prints. The walk is `network_paths::resolve::walk`, the data fence's.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::shape::{Segment, mentions_stetdoc, plain_segments};
use crate::network_paths::fold_name;
use crate::network_paths::resolve::{Disk, Entry, Walked, comparable, walk};
use crate::pre_write_guard::words::Escape;
use crate::stetdoc_args::args_of;

/// The flags that name a fenced place, each followed by one path.
const DATA_FLAG: &str = "--fence-data";
const OWN_FLAG: &str = "--fence-own";
const MATTER_FLAG: &str = "--fence-matter";
const PROTECTED_FLAG: &str = "--fence-protected";
/// The flag saying a fenced place could not be passed ([`GuardFence::overflow`]).
const OVERFLOW_FLAG: &str = "--fence-overflow";

/// The most bytes the other matters' folders take on the hook's command line. Windows starts a
/// program with at most 32 767 characters; past this the list is left out ([`OVERFLOW_FLAG`]).
const MATTERS_BUDGET: usize = 16 * 1024;

/// The commands a line that runs stetdoc may hold beside it, as their first word: filters of
/// its output that start no other program, change no folder and make no link. PowerShell's
/// are matched in any case.
const FILTERS: [&str; 12] = [
    "head", "tail", "grep", "wc", "cut", "tr", "uniq", "echo", "printf", "ls", "pwd", "true",
];
const PS_FILTERS: [&str; 9] = [
    "select-object",
    "select",
    "select-string",
    "sls",
    "out-string",
    "write-output",
    "write-host",
    "measure-object",
    "measure",
];

/// stetdoc v0.9.3's reading subcommands that take a document (`cli.rs`: `ViewArgs` :113,
/// `InspectArgs` :207, `CheckArgs` :376, `LookupArgs` :391), each with its options that take a
/// value (`StampArgs` gives `check` `author` and `date`). The document is the first argument
/// that is no option and no option's value.
const READS: [(&str, &[&str]); 4] = [
    (
        "view",
        &[
            "depth", "range", "clause", "find", "context", "author", "kind", "story", "mode",
            "budget", "after",
        ],
    ),
    ("inspect", &[]),
    ("check", &["author", "date"]),
    ("lookup", &[]),
];

/// The places a launch fences off from stetdoc's reads, passed to the guard as arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GuardFence {
    /// Each spelling of the data folder.
    pub(crate) data: Vec<PathBuf>,
    /// The session's own folders in the data folder (its run folder, the matter's notes, the
    /// built-in skills), which stay readable.
    pub(crate) own: Vec<PathBuf>,
    /// Every other registered matter's folder: the list the Mac sandbox denies
    /// (`agent::loads::sandbox`, `SandboxPaths::other_matters`). When #1085 lands, the matters
    /// this conversation is not invited into.
    pub(crate) matters: Vec<PathBuf>,
    /// The protected roots. Empty until `ps-module-fence` lands
    /// `data_fence::protected_roots`, which fills it.
    pub(crate) protected: Vec<PathBuf>,
    /// A fenced place could not be passed (not UTF-8, or the matters past [`MATTERS_BUDGET`]):
    /// stetdoc may then read only in the matter folder and the own folders.
    pub(crate) overflow: bool,
}

impl GuardFence {
    /// The guard's arguments for this fence: the data folder's, the own folders' and the
    /// protected roots' flags, then the other matters' while they fit [`MATTERS_BUDGET`].
    pub(crate) fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let mut passed = true;
        for path in &self.data {
            passed &= pass(&mut args, DATA_FLAG, path);
        }
        for path in &self.protected {
            passed &= pass(&mut args, PROTECTED_FLAG, path);
        }
        for path in &self.own {
            // An own folder that can't be passed is only more refusals.
            pass(&mut args, OWN_FLAG, path);
        }
        let size: usize = self
            .matters
            .iter()
            .map(|path| path.as_os_str().len() + MATTER_FLAG.len() + 2)
            .sum();
        if size > MATTERS_BUDGET {
            passed = false;
        } else {
            for path in &self.matters {
                passed &= pass(&mut args, MATTER_FLAG, path);
            }
        }
        if self.overflow || !passed {
            args.push(OVERFLOW_FLAG.to_owned());
        }
        args
    }

    /// The fence the guard's arguments `args` pass ([`Self::args`]); empty without any.
    pub(in crate::pre_write_guard) fn from_args(args: &[OsString]) -> Self {
        let mut fence = Self::default();
        let mut words = args.iter();
        while let Some(word) = words.next() {
            let list = match word.to_str() {
                Some(DATA_FLAG) => &mut fence.data,
                Some(OWN_FLAG) => &mut fence.own,
                Some(MATTER_FLAG) => &mut fence.matters,
                Some(PROTECTED_FLAG) => &mut fence.protected,
                Some(OVERFLOW_FLAG) => {
                    fence.overflow = true;
                    continue;
                }
                _ => continue,
            };
            if let Some(path) = words.next() {
                list.push(PathBuf::from(path));
            }
        }
        fence
    }

    /// Whether it fences anything.
    fn is_on(&self) -> bool {
        self.overflow
            || !self.data.is_empty()
            || !self.matters.is_empty()
            || !self.protected.is_empty()
    }
}

/// `flag` and `path` added to `args`; false when `path` is not UTF-8 and so can't go.
fn pass(args: &mut Vec<String>, flag: &str, path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    args.extend([flag.to_owned(), text.to_owned()]);
    true
}

/// Why a stetdoc command is refused, with the word that named the place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::pre_write_guard) enum Fenced {
    /// In the data folder, outside the own folders.
    Data(String),
    /// In another matter's folder (which the agent is never told).
    Matter(String),
    /// In a protected root.
    Protected(String),
    /// Where the word leads could not be found, or the document has more than one name.
    Unchecked(String),
    /// The document does not exist.
    Missing(String),
    /// Outside the matter and the own folders, with the fence cut short.
    Overflow(String),
    /// The line mentions stetdoc and is not the plain shape ([`super::shape`]).
    Shape,
}

/// The first reason `fence` refuses the shell command `command` (its shell's escape `escape`),
/// with `base` the hook's `cwd` and `matter` the session's matter folder, looked at on
/// `disk`. `None` when the fence is empty or the command never mentions stetdoc.
pub(in crate::pre_write_guard) fn fenced_read(
    command: &str,
    escape: Escape,
    (base, matter): (&Path, &Path),
    fence: &GuardFence,
    disk: &Disk<'_>,
) -> Option<Fenced> {
    if !fence.is_on() || !mentions_stetdoc(command) {
        return None;
    }
    let powershell = escape == Escape::Backtick;
    let Some(segments) = plain_segments(command, powershell) else {
        return Some(Fenced::Shape);
    };
    let to = Spot {
        base,
        powershell,
        disk,
    };
    let places = Places::new(fence, matter, disk);
    for segment in &segments {
        if let Some(fenced) = segment_refusal(segment, powershell, &to, &places) {
            return Some(fenced);
        }
    }
    None
}

/// Why `segment` of a plain line is refused, if it is: a stetdoc command fed by a pipe, a word
/// of one leading somewhere fenced, its document missing or hard-linked; any other command not a
/// filter, or with a word that mentions stetdoc.
fn segment_refusal(
    segment: &Segment,
    powershell: bool,
    to: &Spot<'_, '_>,
    places: &Places,
) -> Option<Fenced> {
    let (program, args) = segment.words.split_first()?;
    if program != "stetdoc" {
        let lower = program.to_lowercase();
        let filter = FILTERS.contains(&program.as_str())
            || (powershell
                && (FILTERS.contains(&lower.as_str()) || PS_FILTERS.contains(&lower.as_str())));
        let mentions = segment.words.iter().any(|word| mentions_stetdoc(word));
        return (!filter || mentions).then_some(Fenced::Shape);
    }
    if segment.piped_in {
        return Some(Fenced::Shape);
    }
    let mut literal = false;
    for word in args {
        let value = if literal {
            word.as_str()
        } else if word == "--" {
            literal = true;
            continue;
        } else if word.starts_with('-') {
            match option_value(word) {
                Some(Some(value)) => value,
                Some(None) => continue,
                None => return Some(Fenced::Shape),
            }
        } else {
            word.as_str()
        };
        let Some(real) = to.real(value) else {
            return Some(Fenced::Unchecked(value.to_owned()));
        };
        if let Some(fenced) = places.refuses(&real, value) {
            return Some(fenced);
        }
    }
    let args = args_of(args);
    if let Some((document, _)) = read_words(&args) {
        let Some(real) = to.real(document) else {
            return Some(Fenced::Unchecked(document.to_owned()));
        };
        match to.disk.fs.entry(&real) {
            Ok(Entry::Dir) => {}
            Ok(Entry::File) if to.disk.fs.links(&real).is_ok_and(|links| links == 1) => {}
            Ok(Entry::File | Entry::Link) => return Some(Fenced::Unchecked(document.to_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Some(Fenced::Missing(document.to_owned()));
            }
            Err(_) => return Some(Fenced::Unchecked(document.to_owned())),
        }
    }
    None
}

/// An option word's value: `Some(None)` for a bare option name (`-x`, `--name`),
/// `Some(Some(value))` for `--name=value`, `None` for any other word starting with `-` (a
/// lone `-`, which reads standard input, or an option with something stuck to it).
fn option_value(word: &str) -> Option<Option<&str>> {
    let (name, value) = match word.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (word, None),
    };
    let bare = name.strip_prefix("--").or_else(|| name.strip_prefix('-'))?;
    let mut chars = bare.chars();
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !plain || value.is_some_and(str::is_empty) {
        return None;
    }
    Some(value)
}

/// A reading subcommand's document word among `args` ([`args_of`]) and its other words that are
/// not options; `None` for any other subcommand or with no document.
fn read_words<'a>(args: &[&'a str]) -> Option<(&'a str, Vec<&'a str>)> {
    let (sub, rest) = args.split_first()?;
    let (_, valued) = READS.iter().find(|(name, _)| name == sub)?;
    let mut document = None;
    let mut others = Vec::new();
    let mut rest = rest.iter().copied();
    let mut literal = false;
    while let Some(word) = rest.next() {
        if literal || !word.starts_with('-') || word == "-" {
            if document.is_none() {
                document = Some(word);
            } else {
                others.push(word);
            }
        } else if word == "--" {
            literal = true;
        } else if let Some((_, value)) = word.split_once('=') {
            others.push(value);
        } else if valued.contains(&word.trim_start_matches('-')) {
            others.extend(rest.next());
        }
    }
    Some((document?, others))
}

/// How a word becomes a path.
struct Spot<'d, 'a> {
    /// The hook's `cwd`, for a relative word.
    base: &'a Path,
    /// PowerShell's words, else Bash's.
    powershell: bool,
    disk: &'a Disk<'d>,
}

impl Spot<'_, '_> {
    /// Where the plain word `word` really leads: `None` when it can't be told (an empty word,
    /// a Git Bash form it doesn't map, a path the walk can't vouch for).
    fn real(&self, word: &str) -> Option<PathBuf> {
        if word.is_empty() {
            return None;
        }
        let windows = self.disk.net.windows;
        let text = if self.powershell && !windows {
            // PowerShell takes `\` as a separator on every OS.
            word.replace('\\', "/")
        } else if !self.powershell && windows {
            git_bash(word)?
        } else {
            word.to_owned()
        };
        let path = Path::new(&text);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        };
        match walk(&path, self.disk) {
            // In its firmlinked form, as the fenced places are ([`Places::new`]).
            Walked::Local(real) => Some(self.disk.net.root_form(&real)),
            Walked::Refused => None,
        }
    }
}

/// A Bash word as Git Bash hands it to a Windows program: `/c/rest`, `/cygdrive/c/rest` and
/// `/proc/cygdrive/c/rest` as `C:\rest`; `None` for any other path from its root (`/tmp`,
/// `/usr`, `//server`, mapped to Git's own folders or a share), and for a word with a `:`
/// past a drive's or a `;` (a path list, which it maps item by item).
fn git_bash(word: &str) -> Option<String> {
    let mapped = match word.strip_prefix('/') {
        None => word.to_owned(),
        Some(rest) => {
            let rest = rest
                .strip_prefix("proc/cygdrive/")
                .or_else(|| rest.strip_prefix("cygdrive/"))
                .unwrap_or(rest);
            let mut chars = rest.chars();
            let drive = chars.next().filter(char::is_ascii_alphabetic)?;
            let after = chars.as_str();
            if !(after.is_empty() || after.starts_with('/')) {
                return None;
            }
            let after = after.trim_start_matches('/').replace('/', "\\");
            format!("{}:\\{after}", drive.to_ascii_uppercase())
        }
    };
    let past_drive = mapped.get(2..).unwrap_or_default();
    (!past_drive.contains(':') && !mapped.contains(';')).then_some(mapped)
}

/// The fenced places, each as spelled and as walked, in comparable keys.
struct Places {
    data: Vec<Vec<Key>>,
    own: Vec<Vec<Key>>,
    matters: Vec<Vec<Key>>,
    protected: Vec<Vec<Key>>,
    matter: Vec<Key>,
    overflow: bool,
    fold: bool,
}

/// One path's names, compared.
type Key = Vec<OsString>;

impl Places {
    fn new(fence: &GuardFence, matter: &Path, disk: &Disk<'_>) -> Self {
        let fold = disk.net.fold_case;
        // Each as spelled and as walked, on the Mac in its firmlinked form: a data folder spelled
        // `/System/Volumes/Data/Users/a/x` is `/Users/a/x` (`NetEnv::root_form`).
        let keys = |path: &Path| -> Vec<Key> {
            let mut keys = vec![key(&disk.net.root_form(path), fold)];
            if let Walked::Local(real) = walk(path, disk) {
                let real = key(&disk.net.root_form(&real), fold);
                if !keys.contains(&real) {
                    keys.push(real);
                }
            }
            keys
        };
        let all =
            |paths: &[PathBuf]| -> Vec<Vec<Key>> { paths.iter().map(|path| keys(path)).collect() };
        Self {
            data: all(&fence.data),
            own: all(&fence.own),
            matters: all(&fence.matters),
            protected: all(&fence.protected),
            matter: keys(matter),
            overflow: fence.overflow,
            fold,
        }
    }

    /// Why the document at `real` (walked), named by `word`, is refused, if it is.
    fn refuses(&self, real: &Path, word: &str) -> Option<Fenced> {
        let real = key(real, self.fold);
        let within = |places: &[Key]| places.iter().any(|place| real.starts_with(place));
        let any = |lists: &[Vec<Key>]| lists.iter().any(|list| within(list));
        if any(&self.protected) {
            return Some(Fenced::Protected(word.to_owned()));
        }
        let in_matter = within(&self.matter);
        for keys in &self.matters {
            // A matter that holds this one fences only what lies outside this one.
            let holds_this = keys
                .iter()
                .any(|outer| self.matter.iter().any(|this| this.starts_with(outer)));
            if within(keys) && !(holds_this && in_matter) {
                return Some(Fenced::Matter(word.to_owned()));
            }
        }
        let own = any(&self.own);
        if any(&self.data) && !own {
            return Some(Fenced::Data(word.to_owned()));
        }
        if self.overflow && !in_matter && !own {
            return Some(Fenced::Overflow(word.to_owned()));
        }
        None
    }
}

/// `path`'s names for a comparison (a verbatim drive form put plain), folded when `fold`.
fn key(path: &Path, fold: bool) -> Key {
    comparable(path)
        .components()
        .map(|part| {
            let part = part.as_os_str();
            if fold {
                OsString::from(fold_name(&part.to_string_lossy()))
            } else {
                part.to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "reads_tests.rs"]
mod tests;
