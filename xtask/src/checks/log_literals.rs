//! The `log-literals` check: every `warn!` and `error!` (and `event!` at those levels) in the
//! Windows sandbox's code the outer runner links has a literal message with no format argument:
//! neither `{name}` in the literal, nor an argument after it, nor a `message = ...` field. It
//! reads `crates/sandbox/src/windows/`, but of `windows/setup/` (the setup binary's) only the
//! files the outer calls into ([`SETUP_READ`]: the record's readers, and what they use there).
//!
//! The outer runner writes those two levels to `runner.log` (`windows/runner/log_file.rs`), whose
//! formatter keeps the message as it is: it can't tell a literal from a formatted message, as
//! `tracing` hands both over as `fmt::Arguments` behind `&dyn Debug`. A formatted message could
//! carry a matter's name or a file name into the file, so this check keeps them out at the source;
//! a value goes in a field, which the formatter drops unless its name is one it keeps.
//!
//! It finds the macros by their last path segment, so `tracing::warn!` and an imported `warn!`
//! both count. What it can't follow fails where it is written: a `use` that renames `warn`,
//! `error` or `event` (`use tracing::warn as w`), and a `macro_rules!` whose body calls one of
//! them. An `event!` counts unless its level is a path ending in `TRACE`, `DEBUG` or `INFO`.
//!
//! It reads each such `.rs` file that `git ls-files` lists (tracked, or untracked and not
//! ignored), except tests: a file named `tests.rs` or ending in `_tests.rs`, or under a `tests`
//! folder. It parses each with syn and counts the files it read.

mod calls;
mod fix;
pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use syn::visit::Visit;

/// The folder whose files the check reads, repo-relative.
const DIR: &str = "crates/sandbox/src/windows/";
/// The folder under `DIR` it skips but for [`SETUP_READ`]: the setup binary's.
const SKIPPED: &str = "crates/sandbox/src/windows/setup/";
/// The files of `SKIPPED` the outer runner calls into: `record_values` (`runner/end.rs`,
/// `runner/unclean.rs`), `record_write` (`read_record_install_id`, `read_record_value`), and
/// the two `record_write` uses there.
const SETUP_READ: [&str; 4] = [
    "crates/sandbox/src/windows/setup/record_values.rs",
    "crates/sandbox/src/windows/setup/record_write.rs",
    "crates/sandbox/src/windows/setup/record_key.rs",
    "crates/sandbox/src/windows/setup/lock.rs",
];

/// Fails on a `warn!` or `error!` with a formatted message in the Windows sandbox's code.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "log-literals",
    counts: Noun {
        one: "Rust file",
        many: "Rust files",
    },
    place: || format!("in {DIR}, except {SKIPPED} but its record files, and tests"),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

fn run(ctx: &Context) -> Inspection {
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let mut found = Inspection::default();
    for file in files.iter().filter(|file| is_read(file)) {
        match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => {
                found.inspected += 1;
                found.violations.extend(inspect(file, &text));
            }
            Ok(None) => {}
            Err(violation) => found.violations.push(violation),
        }
    }
    found
}

/// True for a `.rs` file under `DIR`, not under `SKIPPED` unless [`SETUP_READ`] lists it, and
/// not a test file.
fn is_read(file: &str) -> bool {
    let Some(rest) = file.strip_prefix(DIR) else {
        return false;
    };
    let name = rest.rsplit('/').next().unwrap_or(rest);
    let test = name == "tests.rs"
        || name.ends_with("_tests.rs")
        || rest.split('/').any(|part| part == "tests");
    let setup = file.starts_with(SKIPPED) && !SETUP_READ.contains(&file);
    name.ends_with(".rs") && !setup && !test
}

/// The violations of the file `path` holding `text`.
fn inspect(path: &str, text: &str) -> Vec<Violation> {
    let parsed = match syn::parse_file(text) {
        Ok(parsed) => parsed,
        Err(error) => {
            let line = u32::try_from(error.span().start().line).unwrap_or(1).max(1);
            let message = format!("syn cannot parse this file: {error}");
            return vec![checks::violation(path, line, message, Fix::Unparsable)];
        }
    };
    let mut calls = calls::Calls::new(path);
    calls.visit_file(&parsed);
    calls.found
}

#[cfg(test)]
mod tests;
