//! Where stetdoc's reads may go. Claude's `Bash(stetdoc <sub>:*)` allow rules
//! (`clauseo_profile::claude::stetdoc_allowed`) run `view`, `inspect`, `check` and `lookup` with
//! no question, and Claude Code checks no path for stetdoc, so its `Read` and shell deny rules
//! never see the document a command names. stetdoc follows a link given as its input and reads
//! any Word document or working copy the lawyer can read. So the guard reads the input itself:
//! its real path (every link followed, each name's long form, so an 8.3 short name too, by the
//! network-safe walk, [`walk`]), refused when it lies in a fenced place ([`GuardFence`]): the
//! data folder (but the session's own folders in it), another matter's folder, or a protected
//! root. Anything else passes as before: a template in the lawyer's Documents is ordinary use,
//! and stetdoc is Claude's only way to read Word.
//!
//! It fails closed on an input it can't follow: one the walk can't vouch for (a network path, a
//! link it can't read, a name it may not look at), a word the shell would change first (`~`, a
//! `$`, a backtick, a brace or an unquoted glob in Bash), a relative one in a command that
//! changes folder, or a command longer than [`READ_WORDS`] words. Only the form the allow rules
//! match is read: `stetdoc` first in a simple command (after `NAME=value`, `command` and the
//! like), at the top level or in a script handed to `sh -c`, `pwsh -Command` or `eval`. A
//! wrapper (`xargs stetdoc`, `env stetdoc`) matches no rule and goes to a classifier. The
//! writing subcommands ask, as before.
//!
//! The walk is `network_paths::resolve::walk`, the data fence's. When host's shared resolver
//! lands, this switches to it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::super::program_name;
use crate::network_paths::fold_name;
use crate::network_paths::resolve::{Disk, Walked, comparable, walk};
use crate::pre_write_guard::shell::{command_words, is_folder_change, script};
use crate::pre_write_guard::words::{Escape, Segments, shell_segments};
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

/// The most words of a command read; a longer one with a stetdoc read in it is refused.
pub(in crate::pre_write_guard) const READ_WORDS: usize = 1024;

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

/// Why a stetdoc read is refused, with the word that named the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::pre_write_guard) enum Fenced {
    /// In the data folder, outside the own folders.
    Data(String),
    /// In another matter's folder (as the fence spells it).
    Matter(String, PathBuf),
    /// In a protected root.
    Protected(String),
    /// Where the word leads could not be found.
    Unchecked(String),
    /// Outside the matter and the own folders, with the fence cut short.
    Overflow(String),
}

/// The first stetdoc read of the shell command `command` (its shell's escape `escape`) that
/// `fence` refuses, its words taken from `base` (the hook's `cwd`), with `matter` the session's
/// matter folder, looked at on `disk`. `None` when the fence is empty.
pub(in crate::pre_write_guard) fn fenced_read(
    command: &str,
    escape: Escape,
    (base, matter): (&Path, &Path),
    fence: &GuardFence,
    disk: &Disk<'_>,
) -> Option<Fenced> {
    if !fence.is_on() {
        return None;
    }
    let mut top = Segments::new(READ_WORDS);
    shell_segments(command, escape, &mut top);
    let inner: Vec<(Segments, Escape)> = top
        .iter()
        .filter(|segment| segment.command)
        .filter_map(|segment| script(&segment.words, escape))
        .map(|(script, escape)| {
            let mut inner = Segments::new(READ_WORDS);
            shell_segments(&script, escape, &mut inner);
            (inner, escape)
        })
        .collect();
    let all = std::iter::once((&top, escape)).chain(inner.iter().map(|(s, e)| (s, *e)));
    let parts: Vec<_> = all
        .flat_map(|(segments, escape)| {
            let full = segments.full();
            segments
                .iter()
                .filter(|segment| segment.command)
                .map(move |segment| (segment, escape, full))
        })
        .collect();
    let changes = parts
        .iter()
        .any(|(segment, ..)| segment.words.iter().any(|word| is_folder_change(word)));
    // Walked only when a command reads with stetdoc: most shell commands don't.
    let mut places = None;
    for (segment, escape, full) in parts {
        let words = command_words(&segment.words);
        let skipped = segment.words.len() - words.len();
        let Some((program, args)) = words.split_first() else {
            continue;
        };
        if program_name(program) != "stetdoc" {
            continue;
        }
        let args = args_of(args);
        let Some((input, others)) = read_words(&args) else {
            continue;
        };
        if full {
            return Some(Fenced::Unchecked("stetdoc".to_owned()));
        }
        // Where the input stands among the segment's words, for whether it was quoted.
        let quoted = segment
            .words
            .iter()
            .skip(skipped + 1)
            .position(|word| word == input)
            .and_then(|at| segment.quoted.get(skipped + 1 + at).copied())
            .unwrap_or(false);
        let to = Spot {
            base,
            escape,
            changes,
            disk,
        };
        let Some(real) = to.real(input, quoted) else {
            return Some(Fenced::Unchecked(input.to_owned()));
        };
        let places = places.get_or_insert_with(|| Places::new(fence, matter, disk));
        if let Some(fenced) = places.refuses(&real, input) {
            return Some(fenced);
        }
        // Any other word that leads into a fenced place is refused too: a misread option.
        for word in others {
            let found = to
                .real(word, true)
                .and_then(|real| places.refuses(&real, word));
            if let Some(fenced @ (Fenced::Data(_) | Fenced::Matter(..) | Fenced::Protected(_))) =
                found
            {
                return Some(fenced);
            }
        }
    }
    None
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
    /// The hook's `cwd`.
    base: &'a Path,
    escape: Escape,
    /// Whether the command changes folder anywhere.
    changes: bool,
    disk: &'a Disk<'d>,
}

impl Spot<'_, '_> {
    /// Where `word` (`quoted` in part, so a Bash glob in it is literal) really leads: `None` when
    /// it can't be told (see the module doc).
    fn real(&self, word: &str, quoted: bool) -> Option<PathBuf> {
        let bash = self.escape == Escape::Backslash;
        let shell_changes = word.is_empty()
            || word.starts_with('~')
            || word.contains('$')
            || (bash && (word.contains(['`', '{']) || (!quoted && word.contains(['*', '?', '[']))));
        if shell_changes {
            return None;
        }
        let windows = self.disk.net.windows;
        let mut text = word.to_owned();
        if !bash && !windows {
            // PowerShell takes `\` as a separator on every OS.
            text = text.replace('\\', "/");
        }
        if bash && windows {
            // Git Bash hands a native program `/c/x` as `C:\x`.
            text = msys_drive(&text).unwrap_or(text);
        }
        let path = Path::new(&text);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else if self.changes {
            return None;
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

/// Git Bash's `/c` or `/c/rest` as `C:\` or `C:\rest`.
fn msys_drive(text: &str) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    let mut chars = rest.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let after = chars.as_str();
    if !(after.is_empty() || after.starts_with('/')) {
        return None;
    }
    let after = after.trim_start_matches('/').replace('/', "\\");
    Some(format!("{}:\\{after}", drive.to_ascii_uppercase()))
}

/// The fenced places, each as spelled and as walked, in comparable keys.
struct Places {
    data: Vec<Vec<Key>>,
    own: Vec<Vec<Key>>,
    matters: Vec<(PathBuf, Vec<Key>)>,
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
            matters: fence
                .matters
                .iter()
                .map(|path| (path.clone(), keys(path)))
                .collect(),
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
        for (folder, keys) in &self.matters {
            // A matter that holds this one fences only what lies outside this one.
            let holds_this = keys
                .iter()
                .any(|outer| self.matter.iter().any(|this| this.starts_with(outer)));
            if within(keys) && !(holds_this && in_matter) {
                return Some(Fenced::Matter(word.to_owned(), folder.clone()));
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
