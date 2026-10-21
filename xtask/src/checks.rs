//! Static checks: Rust code that inspects the repo instead of running a tool.
//!
//! One file per check in `src/checks/`, registered in `ALL` below. `steps::plan` runs every entry
//! after the tool steps and before nextest.
//!
//! To add a check, copy `checks/budgets.rs` with `checks/budgets/tests.rs`,
//! `checks/budgets/tests/planted.rs` and `checks/budgets/tests/repos.rs`, the smallest check in
//! the full shape: a `CHECK` whose `place` comes from the consts its code reads, a `run` that
//! counts what it inspected, and a `cases!` list of planted cases that `CHECK.planted` points at.
//! Every check keeps that list in `<check>/tests/planted.rs`, each case's failures built with
//! `planted::fails` from the check's `Fix` variants, and the builders of its scratch repos in
//! `<check>/tests/repos.rs` (`Repo::files` writes a repo's files); `planted.rs` says how. Then
//! add it to `ALL`. Its fix lines are a `Fix` enum in `<check>/fix.rs`, one variant per failure
//! kind with the text written once in `Display`, wrapped in `crate::fix::Fix`.
//! `<check>/fix/tests.rs` pins every variant's text with `fix_texts!` (in `crate::fix::tests`),
//! which writes one `match` and numbers its arms. A new variant fails to compile until it has an
//! arm; then `assert_texts` fails, naming the arm, until that arm has exactly one sample, which
//! must print the arm's text.
//!
//! Every check counts what it inspected. `StaticCheck::judge` is the one place that judges a run:
//! it fails on any violation or failure, and on a count of zero, since a check that inspected
//! nothing proves nothing (its inputs moved or vanished). The runner reaches it through
//! `StaticCheck::report` (a check's `run` is private to this module) and every planted case
//! through `planted::verify_named`.

pub(crate) mod budgets;
pub(crate) mod buildtime_edits;
pub(crate) mod capabilities;
pub(crate) mod core_io;
pub(crate) mod crates_table;
pub(crate) mod dependencies;
pub(crate) mod docs;
pub(crate) mod exceptions;
pub(crate) mod file_size;
mod fix;
pub(crate) mod graph;
mod json_text;
pub(crate) mod planted;
pub(crate) mod plugins;
pub(crate) mod tauri_config;
pub(crate) mod toml_lines;

pub(crate) use fix::Fix;

use crate::changes::GitError;
use crate::violation::Violation;
use crate::workspace::{Context, Member};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

/// The repo's cargo config, which cargo reads from the root for every command xtask runs there.
pub(crate) const CARGO_CONFIG: &str = ".cargo/config.toml";

/// A check written in Rust. Run it through `report`, which fails it on any violation and on an
/// inspection of zero items. Only `name` is public: the rest is read here, so nothing outside
/// this module can call `run` and skip the judging.
#[expect(
    clippy::partial_pub_fields,
    reason = "the runner and the plan read the name; the rest stays private so nothing outside \
              checks can call run and skip the judging"
)]
pub(crate) struct StaticCheck {
    pub name: &'static str,
    /// What the check counts, e.g. `capability file`.
    counts: Noun,
    /// Where it looks, as the end of a sentence, e.g. `in the desktop crate's capabilities/`.
    /// Built from the consts the check's code reads, so it cannot go stale when they change.
    place: fn() -> String,
    /// The repo-relative file of the check, `file!()` in its `CHECK`, which the zero-inspected
    /// fix names.
    source: &'static str,
    run: fn(&Context) -> Inspection,
    /// The check's planted cases, the `PLANTED` of its `tests/planted.rs`.
    #[cfg(test)]
    planted: &'static [planted::Planted],
}

/// A noun in the singular and the plural, for counts such as `1 capability file`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Noun {
    pub one: &'static str,
    pub many: &'static str,
}

impl Noun {
    /// `n` and the noun, singular for 1: `1 capability file`, `0 capability files`.
    pub(crate) fn count(self, n: usize) -> String {
        let noun = if n == 1 { self.one } else { self.many };
        format!("{n} {noun}")
    }
}

/// What one run of a check saw: how many items it inspected, what it found wrong, and notes that
/// print but never fail (e.g. a file close to the size limit).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Inspection {
    pub inspected: usize,
    pub violations: Vec<Violation>,
    /// Failures with no `file:line`, such as git failing to list the repo's files.
    pub failures: Vec<Failure>,
    pub notes: Vec<String>,
    /// True when a violation or failure stopped the check before it could look at its items
    /// (`stopped`, `stopped_on`, `checks::unlisted`). False when it looked and found none.
    pub stopped: bool,
}

impl Inspection {
    /// `inspected` items and these violations, with no notes.
    pub(crate) fn new(inspected: usize, violations: Vec<Violation>) -> Inspection {
        Inspection {
            inspected,
            violations,
            failures: Vec::new(),
            notes: Vec::new(),
            stopped: false,
        }
    }

    /// Nothing inspected, because `violation` stopped the check before it could look.
    pub(crate) fn stopped(violation: Violation) -> Inspection {
        Inspection::stopped_on(vec![violation])
    }

    /// Nothing inspected, because `violations` stopped the check before it could look. With no
    /// violation the check did not stop: it looked and found nothing.
    pub(crate) fn stopped_on(violations: Vec<Violation>) -> Inspection {
        Inspection {
            stopped: !violations.is_empty(),
            ..Inspection::new(0, violations)
        }
    }
}

/// A failure with no `file:line`, printed with `Printer::failure`: a check that inspected nothing
/// (the items are missing, not wrong, so there is no line to point at), or a tool the check needs
/// that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    pub message: String,
    pub fix: crate::fix::Fix,
}

impl fmt::Display for Failure {
    /// As `Printer::failure` prints it: `what`, then `    fix: ...`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n    fix: {}", self.message, self.fix)
    }
}

/// A check's run, judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub inspection: Inspection,
    /// `Some` when the check inspected nothing.
    pub nothing_inspected: Option<Failure>,
    /// `1 capability file inspected`, for the result and summary lines.
    pub counted: String,
}

impl Report {
    /// True when the check found a violation or a failure, or inspected nothing.
    pub(crate) fn failed(&self) -> bool {
        !self.inspection.violations.is_empty()
            || !self.inspection.failures.is_empty()
            || self.nothing_inspected.is_some()
    }

    /// Every violation and failure as it prints, in the order the runner prints them.
    #[cfg(test)]
    pub(crate) fn printed(&self) -> Vec<String> {
        let violations = self.inspection.violations.iter().map(ToString::to_string);
        let failures = self
            .inspection
            .failures
            .iter()
            .chain(&self.nothing_inspected);
        violations
            .chain(failures.map(ToString::to_string))
            .collect()
    }
}

impl StaticCheck {
    /// Runs the check on `ctx` and judges the result (see `judge`).
    pub(crate) fn report(&self, ctx: &Context) -> Report {
        self.judge((self.run)(ctx))
    }

    /// Judges one inspection. A count of zero adds the zero-inspected failure, unless the check
    /// says it stopped and printed why. That violation or failure already fails the check and
    /// carries the one fix, and the result line still shows the count of zero.
    fn judge(&self, inspection: Inspection) -> Report {
        let stopped = inspection.stopped
            && (!inspection.violations.is_empty() || !inspection.failures.is_empty());
        let nothing_inspected =
            (inspection.inspected == 0 && !stopped).then(|| self.nothing_inspected());
        Report {
            counted: format!("{} inspected", self.counts.count(inspection.inspected)),
            inspection,
            nothing_inspected,
        }
    }

    /// The failure for an inspection of zero items: `0 capability files inspected in the desktop
    /// crate's capabilities/`. The fix names what the check looks for, where, and its file.
    pub(crate) fn nothing_inspected(&self) -> Failure {
        let place = (self.place)();
        Failure {
            message: format!("{} inspected {place}", self.counts.count(0)),
            fix: Fix::NothingInspected {
                name: self.name,
                many: self.counts.many,
                place,
                source: self.source,
            }
            .into(),
        }
    }

    /// A check for the runner's tests, with no planted cases.
    #[cfg(test)]
    pub(crate) const fn fake(
        name: &'static str,
        counts: Noun,
        place: fn() -> String,
        source: &'static str,
        run: fn(&Context) -> Inspection,
    ) -> StaticCheck {
        StaticCheck {
            name,
            counts,
            place,
            source,
            run,
            planted: &[],
        }
    }
}

/// Every static check, in run order.
pub(crate) const ALL: &[StaticCheck] = &[
    budgets::CHECK,
    capabilities::CHECK,
    plugins::CHECK,
    tauri_config::CHECK,
    buildtime_edits::CHECK,
    dependencies::CHECK,
    core_io::CHECK,
    docs::CHECK,
    file_size::CHECK,
];

/// The package name of the desktop crate. The checks find its directory through `Context::members`,
/// so a moved crate is still checked and a missing one fails instead of passing quietly.
pub(crate) const DESKTOP_PACKAGE: &str = "clauseo-desktop";

/// The workspace members, or the inspection of a check that stops because `cargo metadata`
/// failed: one failure that quotes cargo's error. A check that reads members calls this first,
/// so it never judges an empty workspace that only stands in for a failed load.
pub(crate) fn members(ctx: &Context) -> Result<&[Member], Inspection> {
    ctx.members.as_deref().map_err(|error| Inspection {
        failures: vec![Failure {
            message: format!(
                "could not read the workspace members: {}",
                metadata_quote(error)
            ),
            fix: Fix::Metadata.into(),
        }],
        stopped: true,
        ..Inspection::default()
    })
}

/// What a stopped check quotes of the failed load's `error`. `cargo xtask check` prints the
/// whole error once as a note before the steps, so a check quotes only cargo's first line that
/// starts with `error:` and points up. With no such line it quotes all of the error.
fn metadata_quote(error: &str) -> String {
    match error.lines().find(|line| line.starts_with("error:")) {
        Some(line) => {
            format!("`cargo metadata` failed with {line}. The full error is in the note above")
        }
        None => error.to_owned(),
    }
}

/// The desktop crate's member. When `cargo metadata` failed, the inspection `members` gives;
/// when the crate is not a workspace member, the one violation at `Cargo.toml:1` that every
/// desktop check stops on.
pub(crate) fn desktop_member(ctx: &Context) -> Result<&Member, Inspection> {
    members(ctx)?
        .iter()
        .find(|member| member.name == DESKTOP_PACKAGE)
        .ok_or_else(|| {
            Inspection::stopped(violation(
                "Cargo.toml",
                1,
                format!(
                    "the desktop crate (package `{DESKTOP_PACKAGE}`) is not a workspace member"
                ),
                Fix::DesktopNotMember,
            ))
        })
}

/// The repo-relative directory of the desktop crate, or the inspection `desktop_member` stops on.
pub(crate) fn desktop_dir(ctx: &Context) -> Result<&str, Inspection> {
    desktop_member(ctx).map(|member| member.dir.as_str())
}

/// `a`, `a and b`, `a, b and c`.
pub(crate) fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `dir/name`, or `name` when `dir` is the repo root.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// The inspection of a check that reads the repo's files but could not list them, because
/// `git ls-files` failed. `check` is the check's name, for the fix.
pub(crate) fn unlisted(check: &str, error: &GitError) -> Inspection {
    let check = check.to_owned();
    let fix = match error {
        GitError::NotStarted(_) => Fix::GitNotStarted { check },
        GitError::Failed { .. } => Fix::GitFailed { check },
        GitError::TimedOut(_) => Fix::GitTimedOut { check },
    };
    Inspection {
        failures: vec![Failure {
            message: format!("could not list the repo's files: {error}"),
            fix: fix.into(),
        }],
        stopped: true,
        ..Inspection::default()
    }
}

/// Reads `root/rel` as text. `Ok(None)` when the file does not exist; any other read error is a
/// violation at line 1.
pub(crate) fn read_repo_file(root: &Path, rel: &str) -> Result<Option<String>, Violation> {
    match fs::read_to_string(root.join(rel)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(unreadable(rel, &error)),
    }
}

/// The violation for `path`, which the check could not read. The fix asks for UTF-8 text only
/// when that is the error; otherwise it sends the reader to the OS error in the message.
pub(crate) fn unreadable(path: &str, error: &io::Error) -> Violation {
    let fix = if error.kind() == io::ErrorKind::InvalidData {
        Fix::NotUtf8
    } else {
        Fix::Unreadable
    };
    violation(path, 1, format!("could not read `{path}`: {error}"), fix)
}

/// `path` relative to `root`, with `/` separators.
pub(crate) fn repo_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A violation at `path:line`.
pub(crate) fn violation(
    path: &str,
    line: u32,
    message: impl Into<String>,
    fix: impl Into<crate::fix::Fix>,
) -> Violation {
    Violation {
        path: path.to_owned(),
        line,
        message: message.into(),
        fix: fix.into(),
    }
}

/// The 1-based line number of the line at `index`, counting from 0 as `str::lines` does.
pub(crate) fn line_at(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

/// How many lines `text` has, each ended by `\n`, `\r\n` or a lone `\r` (old Mac endings, which
/// TypeScript, every editor and GitHub break lines on), the last one ended or not. `str::lines`
/// alone would count a file of lone `\r` endings as one line. Bytes, so a caller can count up to
/// any byte, on a char boundary or not.
pub(crate) fn line_count(text: &[u8]) -> usize {
    let mut breaks = 0;
    for (index, byte) in text.iter().enumerate() {
        let crlf = *byte == b'\r' && text.get(index + 1) == Some(&b'\n');
        breaks += usize::from(*byte == b'\n' || (*byte == b'\r' && !crlf));
    }
    let unended = text
        .last()
        .is_some_and(|last| *last != b'\n' && *last != b'\r');
    breaks + usize::from(unended)
}

#[cfg(test)]
mod tests;
