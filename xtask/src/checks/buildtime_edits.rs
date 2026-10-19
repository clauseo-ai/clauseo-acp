//! The `buildtime-edits` check: no edit that `cargo xtask buildtime web` makes for its hot-reload
//! probe is left in the UI sources.
//!
//! The probe writes `data-buildtime="N"` into a file under `ui/app/src` and restores the file when
//! it ends. A run killed half-way (SIGKILL, a closed terminal) leaves the attribute behind until
//! the next web run, and it is easy to commit. Every file under `ui/app/src` is read, bytes that
//! are not UTF-8 replaced, and each line holding `data-buildtime=` fails. The check counts the
//! files it read, so a missing or empty `ui/app/src` inspects nothing and fails.
//!
//! A live run holds the edit for real. So when the attribute is found, the check tries the
//! buildtime lock (`target/xtask/buildtime.lock`, released at once): when another process holds
//! it, the violation says a run is in progress instead of calling the edit a leftover.

use crate::buildtime::lock;
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The UI sources the probe edits, relative to the repo root.
const SRC: &str = "ui/app/src";

/// The text the probe writes.
const MARKER: &str = "data-buildtime=";

const MESSAGE: &str = "an interrupted `cargo xtask buildtime web` left its edit here";
const FIX: &str = "run `cargo xtask buildtime web` once to restore the file from target/xtask/buildtime, or remove the attribute by hand";
const LIVE_MESSAGE: &str = "a `cargo xtask buildtime` run is in progress and holds this edit";
const LIVE_FIX: &str = "rerun the check when that run ends";

/// Fails on every line under `ui/app/src` that holds the probe's attribute.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "buildtime-edits",
    counts: Noun {
        one: "UI source file",
        many: "UI source files",
    },
    place: || format!("under {SRC}"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

fn run(ctx: &Context) -> Inspection {
    let mut files = Vec::new();
    if let Err(error) = collect(&ctx.root.join(SRC), &mut files) {
        return Inspection::stopped(checks::unreadable(SRC, &error));
    }
    let mut read = 0;
    files.sort();
    let mut found = Vec::new();
    let mut held = None;
    for file in &files {
        let path = checks::repo_path(&ctx.root, file);
        match fs::read(file) {
            Ok(bytes) => {
                read += 1;
                let text = String::from_utf8_lossy(&bytes);
                if text.contains(MARKER) {
                    let held =
                        *held.get_or_insert_with(|| lock::is_held(&ctx.root.join(lock::LOCK_PATH)));
                    found.extend(scan(&path, &text, held));
                }
            }
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    Inspection::new(read, found)
}

/// Every file under `dir`, recursively. A missing `dir` adds none.
fn collect(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// One violation for each line of `text`, the file at `path`, that holds the probe's attribute,
/// worded for a live run when `held` (the buildtime lock is held), else for a leftover.
fn scan(path: &str, text: &str, held: bool) -> Vec<Violation> {
    let (message, fix) = if held {
        (LIVE_MESSAGE, LIVE_FIX)
    } else {
        (MESSAGE, FIX)
    };
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(MARKER))
        .map(|(index, _)| checks::violation(path, checks::line_at(index), message, fix))
        .collect()
}

#[cfg(test)]
mod tests;
