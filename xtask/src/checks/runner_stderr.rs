//! The `runner-stderr` check: the Windows sandbox runner binary writes to stderr only through its
//! `stderr` module (`crates/sandbox/src/windows/runner/stderr.rs`), whose `refuse` and `notice`
//! print one fixed sentence each, and whose `agent_stream` relays the agent's stderr. The files it
//! reads are `crates/sandbox/src/windows/runner.rs`, each `.rs` file under
//! `crates/sandbox/src/windows/runner/`, and `crates/sandbox/src/bin/clauseo-sandbox-run.rs`.
//!
//! The host copies a tail of the runner's stderr into the matter's event log, so the runner
//! prints only `ERR/NOTE clauseo-sandbox-run kind=<kind>: <fixed sentence>` lines, through
//! `runner::stderr`. Error text can hold a path, so an error's Display or Debug is formatted only
//! inside a `Detail::new(|| ...)` argument, a closure that only the `vm-harness-detail` feature's
//! builds call. This check fails:
//!
//! - a macro that writes to the console: `eprintln!`, `eprint!`, `println!`, `print!` or `dbg!`;
//! - a `write!` or `writeln!` whose target mentions `stderr` (`stderr`, `io::stderr()`,
//!   `self.stderr`);
//! - a call of a path ending in `stderr` (`io::stderr()`, `stderr()`), anywhere, closures included;
//! - an error formatted outside a `Detail::new` argument: a placeholder or a positional argument
//!   of a format macro (`format!`, `write!`, `panic!`, `assert!` and the like) that names an
//!   error (`e`, `err`, `error`, or a name ending in `_err` or `_error`), or a `.to_string()`
//!   on such a name;
//! - a `use` that renames `Detail` or `stderr`, and a `macro_rules!` whose body calls a console
//!   macro or names `stderr`: what the check can't follow, it fails where it is written;
//! - a file syn cannot parse, at the line it stopped on.
//!
//! It finds the macros by their last path segment, so `std::println!` counts. It reads each such
//! `.rs` file that `git ls-files` lists (tracked, or untracked and not ignored), except tests: a
//! file named `tests.rs` or ending in `_tests.rs`, or under a `tests` folder. Inside a file it
//! also skips each inline `#[cfg(test)] mod` (only exactly `cfg(test)`: `cfg(not(test))` and
//! `cfg(any(test, ..))` are read), since a test's fake can format an error. It skips the API
//! module itself, `runner/stderr.rs` and `runner/stderr/`, which is where the writes are. It
//! parses each file it reads with syn and counts the files it read.

mod calls;
mod fix;
pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::path::Path;
use syn::visit::Visit;

/// The runner's root file, repo-relative.
const RUNNER: &str = "crates/sandbox/src/windows/runner.rs";
/// The folder of the runner's modules, repo-relative.
const RUNNER_DIR: &str = "crates/sandbox/src/windows/runner/";
/// The runner binary's entry point, repo-relative.
const BIN: &str = "crates/sandbox/src/bin/clauseo-sandbox-run.rs";
/// The API module the check skips: the only place that writes to stderr.
const API_FILE: &str = "crates/sandbox/src/windows/runner/stderr.rs";
/// The folder of the API module's submodules, skipped with it.
const API_DIR: &str = "crates/sandbox/src/windows/runner/stderr/";
/// The folder every read file is under, stripped before the test-folder rule looks at it.
const SRC: &str = "crates/sandbox/src/";

/// Fails on a console write, a stderr handle, or an error's text formatted outside `Detail::new`
/// in the Windows sandbox's runner.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "runner-stderr",
    counts: Noun {
        one: "Rust file",
        many: "Rust files",
    },
    place: || {
        format!(
            "in {RUNNER}, {RUNNER_DIR} and {BIN}, except {API_DIR}, tests and #[cfg(test)] modules"
        )
    },
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

/// True for a `.rs` file of the runner or its binary, not the API module and not a test file.
fn is_read(file: &str) -> bool {
    if file != RUNNER && file != BIN && !file.starts_with(RUNNER_DIR) {
        return false;
    }
    if file == API_FILE || file.starts_with(API_DIR) {
        return false;
    }
    let rest = file.strip_prefix(SRC).unwrap_or(file);
    let name = rest.rsplit('/').next().unwrap_or(rest);
    let test = name == "tests.rs"
        || name.ends_with("_tests.rs")
        || rest.split('/').any(|part| part == "tests");
    Path::new(name).extension().is_some_and(|ext| ext == "rs") && !test
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
