//! The `runner-stderr` check: the Windows sandbox runner binary writes to stderr only through its
//! `stderr` module (`crates/sandbox/src/windows/runner/stderr.rs`), whose `refuse` and `notice`
//! print one fixed sentence each, and whose `agent_stream` relays the agent's stderr. The runner's
//! files are `crates/sandbox/src/windows/runner.rs`, each `.rs` file under
//! `crates/sandbox/src/windows/runner/`, and `crates/sandbox/src/bin/clauseo-sandbox-run.rs`.
//! Its helpers are the rest of `crate::windows` (`crates/sandbox/src/windows.rs` and the files
//! under `crates/sandbox/src/windows/`), and `crate::denial` and `crate::detect`, whose types the
//! runner names: the runner imports only from those modules, or a name the crate root (`lib.rs`)
//! re-exports from one of them, which this check enforces, so a helper it calls is among them.
//! The setup binary
//! (`crates/sandbox/src/bin/clauseo-sandbox-setup.rs`) is not read.
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
//! - `io::stderr` as a value, passed or stored rather than called: a path of two or more segments
//!   ending in `stderr`, and a `use` that imports `io::stderr` (or globs `io::*`), which would
//!   let a bare `stderr` name it;
//! - in the runner's files, a `use crate::` (or `clauseo_sandbox::`) of anything but those
//!   modules or a name `lib.rs` re-exports from them (`pub use denial::{Denial, ..}`): a helper
//!   there would be outside what the check reads;
//! - an error formatted outside a `Detail::new` argument: a placeholder or a positional argument
//!   of a format macro (`format!`, `write!`, `panic!`, `assert!` and the like) that names an
//!   error (`e`, `err`, `error`, or a name ending in `_err` or `_error`), or a `.to_string()`
//!   on such a name;
//! - a `use` that renames `Detail` or `stderr`, and a `macro_rules!` whose body calls a console
//!   macro or names `stderr`: what the check can't follow, it fails where it is written;
//! - a file syn cannot parse, at the line it stopped on.
//!
//! In a helper it fails only the writes to stderr (the console macros, `write!` to stderr,
//! `stderr` called or as a value, a `use` that renames `stderr`, a `macro_rules!` that writes):
//! a helper's error text reaches stderr only through the runner, where the error rules apply.
//!
//! It finds the macros by their last path segment, so `std::println!` counts. It reads each such
//! `.rs` file that `git ls-files` lists (tracked, or untracked and not ignored), except tests: a
//! file named `tests.rs` or ending in `_tests.rs`, or under a `tests` folder. Inside a file it
//! also skips each inline `#[cfg(test)] mod` (only exactly `cfg(test)`: `cfg(not(test))` and
//! `cfg(any(test, ..))` are read), since a test's fake can format an error. It skips the API
//! module itself, `runner/stderr.rs` and `runner/stderr/`, which is where the writes are. It
//! parses each file it reads with syn and counts the files it read. Of the helpers it also skips
//! each file of a module declared under exactly `#[cfg(test)]` (`#[cfg(test)] mod test_pool;` in
//! `windows.rs` skips `windows/test_pool.rs` and everything under `windows/test_pool/`).

mod calls;
mod fix;
mod imports;
pub(crate) use fix::Fix;

use crate::changes;
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::collections::{BTreeSet, HashMap};
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
/// The crate's root file, whose `pub use` items name what the runner may import from the root.
const LIB: &str = "crates/sandbox/src/lib.rs";
/// The crate's top modules whose files are the runner's helpers: `crate::windows`, and the two
/// whose types it names. Each is `crates/sandbox/src/<module>.rs` and the files under
/// `crates/sandbox/src/<module>/`.
pub(super) const HELPER_MODULES: [&str; 3] = ["windows", "denial", "detect"];

/// What a read file is to the check: the runner's own, or a helper in `crate::windows`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// The runner's modules and its binary: every rule.
    Runner,
    /// The rest of `crate::windows`: the stderr writes only.
    Helper,
}

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
            "in {RUNNER}, {RUNNER_DIR} and {BIN}, and the stderr writes of their helpers (the \
             modules {} under {SRC}), except {API_DIR}, tests and #[cfg(test)] modules",
            HELPER_MODULES.join(", ")
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
    let exports = match checks::read_repo_file(&ctx.root, LIB) {
        Ok(text) => root_exports(text.as_deref().unwrap_or_default()),
        Err(violation) => {
            found.violations.push(violation);
            BTreeSet::new()
        }
    };
    let mut parents = HashMap::new();
    for file in &files {
        let Some(scope) = scope_of(file) else {
            continue;
        };
        if scope == Scope::Helper && test_gated(&ctx.root, file, &mut parents) {
            continue;
        }
        match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => {
                found.inspected += 1;
                found
                    .violations
                    .extend(inspect_with(file, &text, scope, &exports));
            }
            Ok(None) => {}
            Err(violation) => found.violations.push(violation),
        }
    }
    found
}

/// The scope of a `.rs` file the check reads: the runner's or its binary's, or a helper's in
/// `crate::windows`. `None` for the API module, a test file and anything else.
fn scope_of(file: &str) -> Option<Scope> {
    if file == API_FILE || file.starts_with(API_DIR) {
        return None;
    }
    let rest = file.strip_prefix(SRC).unwrap_or(file);
    let name = rest.rsplit('/').next().unwrap_or(rest);
    let test = name == "tests.rs"
        || name.ends_with("_tests.rs")
        || rest.split('/').any(|part| part == "tests");
    if test || Path::new(name).extension().is_none_or(|ext| ext != "rs") {
        return None;
    }
    if file == RUNNER || file == BIN || file.starts_with(RUNNER_DIR) {
        Some(Scope::Runner)
    } else if HELPER_MODULES
        .iter()
        .any(|module| rest == format!("{module}.rs") || rest.starts_with(&format!("{module}/")))
    {
        Some(Scope::Helper)
    } else {
        None
    }
}

/// True when `file` (under `crates/sandbox/src/`) belongs to a module declared under exactly
/// `#[cfg(test)]`: some ancestor module's file (`<parent>.rs` or `<parent>/mod.rs`) holds
/// `#[cfg(test)] mod <child>;`. `parents` keeps each parent file parsed (`None`: absent or not
/// Rust syn can parse, which declares nothing).
fn test_gated(root: &Path, file: &str, parents: &mut HashMap<String, Option<syn::File>>) -> bool {
    let Some(rest) = file
        .strip_prefix(SRC)
        .and_then(|rest| rest.strip_suffix(".rs"))
    else {
        return false;
    };
    let mut parts: Vec<&str> = rest.split('/').collect();
    if parts.last() == Some(&"mod") {
        parts.pop();
    }
    (1..parts.len()).any(|depth| {
        let parent = parts[..depth].join("/");
        let child = parts[depth];
        [format!("{SRC}{parent}.rs"), format!("{SRC}{parent}/mod.rs")]
            .into_iter()
            .any(|candidate| {
                parents
                    .entry(candidate)
                    .or_insert_with_key(|candidate| {
                        checks::read_repo_file(root, candidate)
                            .ok()
                            .flatten()
                            .and_then(|text| syn::parse_file(&text).ok())
                    })
                    .as_ref()
                    .is_some_and(|parsed| declares_test_module(parsed, child))
            })
    })
}

/// True when `parsed` declares the out-of-line module `name` under exactly `#[cfg(test)]`.
fn declares_test_module(parsed: &syn::File, name: &str) -> bool {
    parsed.items.iter().any(|item| {
        matches!(item, syn::Item::Mod(module)
            if module.ident == name && module.content.is_none() && calls::is_cfg_test(&module.attrs))
    })
}

/// The names the crate root re-exports from a helper module: each leaf of a `pub use` in `lib`
/// (the root file's text) whose path starts with one of [`HELPER_MODULES`]. Text syn can't parse
/// exports nothing.
fn root_exports(lib: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let Ok(parsed) = syn::parse_file(lib) else {
        return names;
    };
    for item in &parsed.items {
        if let syn::Item::Use(used) = item
            && matches!(used.vis, syn::Visibility::Public(_))
            && let syn::UseTree::Path(path) = &used.tree
            && HELPER_MODULES.iter().any(|module| path.ident == module)
        {
            imports::leaves(&path.tree, &mut names);
        }
    }
    names
}

/// The violations of the file `path` holding `text`, read as `scope`, with no root re-exports.
#[cfg(test)]
fn inspect(path: &str, text: &str, scope: Scope) -> Vec<Violation> {
    inspect_with(path, text, scope, &BTreeSet::new())
}

/// The violations of the file `path` holding `text`, read as `scope`; `exports` are the names
/// the runner may import from the crate root ([`root_exports`]).
fn inspect_with(
    path: &str,
    text: &str,
    scope: Scope,
    exports: &BTreeSet<String>,
) -> Vec<Violation> {
    let parsed = match syn::parse_file(text) {
        Ok(parsed) => parsed,
        Err(error) => {
            let line = u32::try_from(error.span().start().line).unwrap_or(1).max(1);
            let message = format!("syn cannot parse this file: {error}");
            return vec![checks::violation(path, line, message, Fix::Unparsable)];
        }
    };
    let mut calls = calls::Calls::new(path, scope, exports);
    calls.visit_file(&parsed);
    calls.found
}

#[cfg(test)]
mod tests;
