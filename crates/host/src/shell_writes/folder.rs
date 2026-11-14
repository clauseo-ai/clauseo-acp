//! The folder each segment of a shell command runs in, for [`super::add_script`]: a `cd` or
//! `pushd` to a plain folder (PowerShell's `Set-Location`, `sl`, `chdir` and `Push-Location`
//! too, `-Path`/`-LiteralPath` or not), wherever it stands, sets the folder of the segments
//! after it when `&&`, `;` or a new line joins it to them. A folder change that may not run in
//! this shell or may not run at all leaves the folder unknown for everything after it: one in a
//! pipeline, one followed by `||` or `&`, one inside `( … )` or `{ …; }`, one to a folder with
//! an expansion, a `popd` or `Pop-Location`, or a `cd` that ended an `&&` chain once that chain
//! ends. An unknown folder drops the relative targets after it, not the absolute ones; a later
//! change to an absolute folder makes it known again.

use super::{Dialect, Segment, Sep, Word, is_absolute, join};

/// A command that changes the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// `cd` or `pushd` (PowerShell's `Set-Location`, `sl`, `chdir` and `Push-Location` too).
    To,
    /// `popd` (PowerShell's `Pop-Location` too).
    Back,
}

/// Whether the program word `first` changes the folder, in `dialect` (PowerShell's names in
/// any case).
fn folder_change(first: &str, dialect: Dialect) -> Option<Change> {
    let name = match dialect {
        Dialect::Posix => first.to_owned(),
        Dialect::PowerShell => first.to_ascii_lowercase(),
    };
    match (dialect, name.as_str()) {
        (_, "cd" | "pushd")
        | (Dialect::PowerShell, "chdir" | "sl" | "set-location" | "push-location") => {
            Some(Change::To)
        }
        (_, "popd") | (Dialect::PowerShell, "pop-location") => Some(Change::Back),
        _ => None,
    }
}

/// The folder of the segment being read, as the segments before it left it.
pub(super) struct Folder {
    /// Relative to the matter's folder (`""`) or absolute; `None` when unknown.
    now: Option<String>,
    /// The folder was set by a `cd` after `&&`, which the chain's end leaves unknown.
    conditional: bool,
    /// What joins the segment before to the one being read.
    before: Sep,
    /// How many `{ …; }` groups are open.
    groups: usize,
}

impl Folder {
    /// The folder of a command run in `cwd` (`""`: the matter's folder).
    pub(super) fn new(cwd: &str) -> Self {
        Self {
            now: Some(cwd.to_owned()),
            conditional: false,
            before: Sep::Then,
            groups: 0,
        }
    }

    /// Reads one segment: the folder it runs in and the words of its command (a group's
    /// opening braces taken off), or `None` when it changes the folder, which writes nothing.
    pub(super) fn read<'a>(
        &mut self,
        segment: &'a Segment,
        dialect: Dialect,
    ) -> Option<(Option<String>, &'a [Word])> {
        let opened = segment
            .words
            .iter()
            .take_while(|word| word.text == "{")
            .count();
        self.groups += opened;
        let words = segment.words.get(opened..).unwrap_or_default();
        let change = words
            .first()
            .and_then(|first| folder_change(&first.text, dialect));
        let result = match change {
            Some(change) => {
                self.change(change, words, segment, dialect);
                None
            }
            None => Some((self.now.clone(), words)),
        };
        let closed = words.iter().filter(|word| word.text == "}").count();
        self.groups = self.groups.saturating_sub(closed);
        self.before = segment.ender;
        if segment.ender == Sep::Then && std::mem::take(&mut self.conditional) {
            self.now = None;
        }
        result
    }

    fn change(&mut self, change: Change, words: &[Word], segment: &Segment, dialect: Dialect) {
        let words = match (dialect, words) {
            (Dialect::PowerShell, [first, flag, rest @ ..])
                if ["-path", "-literalpath"].contains(&flag.text.to_ascii_lowercase().as_str()) =>
            {
                std::iter::once(first).chain(rest).collect::<Vec<_>>()
            }
            (_, words) => words.iter().collect(),
        };
        let runs_here = segment.redirects.is_empty()
            && !segment.nested
            && self.groups == 0
            && matches!(self.before, Sep::Then | Sep::And)
            && matches!(segment.ender, Sep::Then | Sep::And | Sep::End);
        self.now = match (change, words.as_slice()) {
            (Change::To, [_, dir]) if runs_here && dir.clear => match &self.now {
                Some(now) => join(now, &dir.text),
                None if is_absolute(&dir.text) => join("", &dir.text),
                None => None,
            },
            _ => None,
        };
        self.conditional = self.now.is_some() && self.before == Sep::And;
    }
}
