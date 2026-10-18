//! Static checks: Rust code that inspects the repo instead of running a tool.
//!
//! One file per check in `src/checks/`, registered in `ALL` below. `steps::plan` runs every entry
//! after the tool steps and before nextest.
//!
//! Every check counts what it inspected. `StaticCheck::report` is the one place that judges a run:
//! it fails on any violation, and on a count of zero, since a check that inspected nothing proves
//! nothing (its inputs moved or vanished). The runner and the tests both go through it.

mod budgets;
mod buildtime_edits;
mod capabilities;
mod core_io;
pub(crate) mod crates_table;
mod dependencies;
mod docs;
pub(crate) mod exceptions;
mod file_size;
mod json_text;
pub(crate) mod planted;
mod plugins;
mod tauri_config;
pub(crate) mod toml_lines;

use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::Path;

/// A check written in Rust. Run it through `report`, which fails it on any violation and on an
/// inspection of zero items.
pub(crate) struct StaticCheck {
    pub name: &'static str,
    /// What the check counts, e.g. `capability file`.
    pub counts: Noun,
    /// Where it looks, as the end of a sentence, e.g. `in the desktop crate's capabilities/`.
    pub place: &'static str,
    pub run: fn(&Context) -> Inspection,
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
    pub notes: Vec<String>,
}

impl Inspection {
    /// `inspected` items and these violations, with no notes.
    pub(crate) fn new(inspected: usize, violations: Vec<Violation>) -> Inspection {
        Inspection {
            inspected,
            violations,
            notes: Vec::new(),
        }
    }

    /// Nothing inspected, because `violation` stopped the check before it could look.
    pub(crate) fn stopped(violation: Violation) -> Inspection {
        Inspection::new(0, vec![violation])
    }
}

/// The failure of a check that inspected nothing. It has no `file:line`: the items are missing,
/// not wrong, so there is no line to point at. Printed with `Printer::failure`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NothingInspected {
    pub message: String,
    pub fix: String,
}

/// A check's run, judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub inspection: Inspection,
    /// `Some` when the check inspected nothing.
    pub nothing_inspected: Option<NothingInspected>,
    /// `1 capability file inspected`, for the result and summary lines.
    pub counted: String,
}

impl Report {
    /// True when the check found a violation or inspected nothing.
    pub(crate) fn failed(&self) -> bool {
        !self.inspection.violations.is_empty() || self.nothing_inspected.is_some()
    }
}

impl StaticCheck {
    /// Runs the check on `ctx` and judges the result (see `judge`).
    pub(crate) fn report(&self, ctx: &Context) -> Report {
        self.judge((self.run)(ctx))
    }

    /// Judges one inspection: a count of zero adds the `NothingInspected` failure.
    pub(crate) fn judge(&self, inspection: Inspection) -> Report {
        let nothing_inspected = (inspection.inspected == 0).then(|| self.nothing_inspected());
        Report {
            counted: format!("{} inspected", self.counts.count(inspection.inspected)),
            inspection,
            nothing_inspected,
        }
    }

    /// The failure for an inspection of zero items: `0 capability files inspected in the desktop
    /// crate's capabilities/`, with a fix that names what the check looks for, where, and its file.
    pub(crate) fn nothing_inspected(&self) -> NothingInspected {
        NothingInspected {
            message: format!("{} inspected {}", self.counts.count(0), self.place),
            fix: format!(
                "{name} looks for {many} {place}; restore them, or update {source} if they moved",
                name = self.name,
                many = self.counts.many,
                place = self.place,
                source = self.source(),
            ),
        }
    }

    /// The repo-relative file that defines the check: `xtask/src/checks/<name>.rs`, `-` as `_`.
    pub(crate) fn source(&self) -> String {
        format!("xtask/src/checks/{}.rs", self.name.replace('-', "_"))
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

/// The repo-relative directory of the desktop crate. When it is not a workspace member, the one
/// violation at `Cargo.toml:1` that every desktop check reports.
pub(crate) fn desktop_dir(ctx: &Context) -> Result<&str, Violation> {
    ctx.members
        .iter()
        .find(|member| member.name == DESKTOP_PACKAGE)
        .map(|member| member.dir.as_str())
        .ok_or_else(|| {
            violation(
                "Cargo.toml",
                1,
                format!("the desktop crate (package `{DESKTOP_PACKAGE}`) is not a workspace member"),
                "restore crates/desktop or update xtask/src/checks: these checks guard its capabilities, plugins and Tauri config",
            )
        })
}

/// `dir/name`, or `name` when `dir` is the repo root.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// `text` with everything from `//` to the end of each line removed, line count kept. A simple
/// per-line strip: it does not know string literals (a `"//"` in a string cuts the line there) or
/// block comments. A cut line can hide a call: a required call then fails the check, but a
/// forbidden one after the cut passes, so the scan is best effort.
pub(crate) fn strip_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.find("//")
                .map_or(line, |start| line.get(..start).unwrap_or(line))
        })
        .collect::<Vec<_>>()
        .join("\n")
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

/// The violation for `path`, which exists but cannot be read as UTF-8 text.
pub(crate) fn unreadable(path: &str, error: &io::Error) -> Violation {
    Violation {
        path: path.to_owned(),
        line: 1,
        message: format!("could not read `{path}`: {error}"),
        fix: "make the file readable as UTF-8 text".to_owned(),
    }
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
    fix: impl Into<String>,
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

#[cfg(test)]
mod tests;
