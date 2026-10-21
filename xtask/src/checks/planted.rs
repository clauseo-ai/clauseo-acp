//! Planted violations: scratch repos a check must fail on (the ways around it included) and repos
//! it must pass, kept as one list per check, `StaticCheck::planted`, which is the registry.
//!
//! Every check keeps them in the same two files, out of its production file:
//!
//! - `<check>/tests/planted.rs` holds the one `cases!` list, declared from `<check>/tests.rs`.
//!   `cases!` turns the list into `PLANTED`, a `&[Planted]`, and into one `#[test]` per case, so
//!   `cargo test -p xtask planted` lists `checks::<check>::tests::planted::<case>`. Never put a
//!   function there: each case's test takes its name in that module.
//! - `<check>/tests/repos.rs` holds the functions that build each case's repo, and nothing else
//!   the cases do not call. Past 500 lines it keeps the shared helpers and moves the builders by
//!   topic into `<check>/tests/repos/<topic>.rs`, re-exported with `pub(super) use <topic>::*`,
//!   so every case still names `super::repos::<builder>`.
//!
//! ```ignore
//! use crate::checks::planted::fails;
//! use crate::budgets::Fix;
//!
//! crate::checks::planted::cases! {
//!     /// A row whose limit is below zero: the one row is read, and it fails.
//!     negative_limit: Fails[fails(
//!         "budgets.toml:4",
//!         "budget window_visible: limit must be zero or more",
//!         Fix::NegativeLimit,
//!     )], inspects 1 => super::repos::negative_limit,
//!     valid_row: Passes, inspects 1 => super::repos::valid_row,
//! }
//! ```
//!
//! The expectation is a variant of `Expect`, written without the type. `Fails[...]` lists each
//! failure as it prints, built by `fails` (`path:line`, message and fix) or `failure` (no
//! `path:line`). The location and the message are spelled out; the fix is the check's `Fix`
//! variant, so its text lives once, in the `Display` that `<check>/fix/tests.rs` pins, and the
//! harness still compares the exact printed lines. `Fails(path)` takes a function instead, for
//! lines that differ by OS.
//!
//! Every case also states `inspects N`: how many items its check must report inspecting
//! (`Inspection::inspected`). N is worked out from what the case's builder writes (the files,
//! rows or crates the check reads), not copied from a run, so a check that counts what it never
//! read fails its cases.
//!
//! A builder takes no argument and returns a `Repo`: a scratch directory holding the case's one
//! violation (or none) and the `Context` the check runs in. `Repo::files` writes the files of a
//! repo with no workspace member; `scratch` writes them and returns the directory, for a builder
//! that adds members, git or cargo. A builder never runs the check. The harness does, through
//! `StaticCheck::report`, the same call `cargo xtask check` makes, so no case can reach an inner
//! function and skip what the whole check does: what it reads, how it counts and its
//! zero-inspected judging. Each case runs once, in its own test; the registry test in `tests.rs`
//! runs nothing and holds `ALL` to the rules: every check has cases, a failing one and a passing
//! one among them, and its `CHECK.planted` is the list of its own `tests/planted.rs`.

#[cfg(test)]
mod repo;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use repo::{Repo, git, scratch};

/// What a planted case must produce, judged by the check's own `StaticCheck::judge`.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum Expect {
    /// Exactly these violations and failures, each as it prints (`path:line: what\n    fix: ...`,
    /// or `what\n    fix: ...` for one without a location), in the order the runner prints them.
    /// The zero-inspected failure is one of them when the case inspected nothing. The lines are
    /// built when the case runs, by `fails` and `failure`, so each fix is named by its `Fix`
    /// variant. `cases!` writes `Fails[line, ...]` as a closure.
    Fails(fn() -> Vec<String>),
    /// The check passes: no violation, no failure, and at least one item inspected.
    Passes,
}

/// A violation as it prints: `{at}: {message}`, then `    fix: ` and the fix's text. `at` is
/// `path:line`. The fix is a check's `Fix` variant, never its text, so a fix is worded in one
/// place.
#[cfg(test)]
pub(crate) fn fails(at: &str, message: &str, fix: impl Into<Fix>) -> String {
    format!("{at}: {message}\n    fix: {}", fix.into())
}

/// A failure with no `path:line` as it prints: `message`, then `    fix: ` and the fix's text.
#[cfg(test)]
pub(crate) fn failure(message: &str, fix: impl Into<Fix>) -> String {
    format!("{message}\n    fix: {}", fix.into())
}

/// One planted case of a check.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Planted {
    /// The case's name, also the name of its test.
    pub name: &'static str,
    /// The file whose `cases!` list holds the case, `file!()` there, which the registry
    /// matches with its check's file.
    pub file: &'static str,
    pub expect: Expect,
    /// How many items the check must report inspecting for this input.
    pub inspects: usize,
    /// Builds the scratch repo the check runs on.
    pub repo: fn() -> Repo,
}

#[cfg(test)]
impl Planted {
    /// Builds the case's repo and runs `check` on it as the runner does.
    pub(crate) fn report(&self, check: &StaticCheck) -> Report {
        let repo = (self.repo)();
        check.report(repo.ctx())
    }

    /// Runs the case and judges it as `check`. `Err` says how the result differs from `expect`
    /// and `inspects`; a count that differs is reported first.
    pub(crate) fn verdict(&self, check: &StaticCheck) -> Result<(), String> {
        let report = self.report(check);
        let printed = report.printed();
        if self.inspects != report.inspection.inspected {
            return Err(format!(
                "expected {} inspected, found {}",
                self.inspects, report.inspection.inspected
            ));
        }
        let fixes = report.inspection.violations.iter().map(|found| &found.fix);
        let mut fixes = fixes.chain(
            report
                .inspection
                .failures
                .iter()
                .map(|failure| &failure.fix),
        );
        if let Some(text) = fixes.find(|fix| matches!(fix, Fix::Tool(_))) {
            return Err(format!(
                "the fix `{text}` is a step's text; give it a variant in the check's Fix enum"
            ));
        }
        let expected: Vec<String> = match self.expect {
            Expect::Passes if report.failed() => {
                return Err(format!("expected a pass, found:\n{}", printed.join("\n")));
            }
            Expect::Passes => return Ok(()),
            Expect::Fails(lines) => lines(),
        };
        if expected.is_empty() {
            return Err(
                "a failing case lists at least one failure; a case that passes is Passes"
                    .to_owned(),
            );
        }
        if printed != expected {
            return Err(format!(
                "expected these failures:\n{}\nfound:\n{}",
                expected.join("\n"),
                printed.join("\n")
            ));
        }
        Ok(())
    }

    /// True for a case that expects the check to pass.
    fn passes(&self) -> bool {
        matches!(self.expect, Expect::Passes)
    }
}

/// Runs the case called `name` in `check.planted` and panics with its verdict when it fails.
/// The body of every test `cases!` makes, so a case in a list that `CHECK.planted` does not
/// point at fails too.
#[cfg(test)]
pub(crate) fn verify_named(check: &StaticCheck, name: &str) {
    let Some(case) = check.planted.iter().find(|case| case.name == name) else {
        panic!(
            "no planted case `{name}` in the planted list of {}",
            check.name
        );
    };
    if let Err(why) = case.verdict(check) {
        panic!("planted case `{name}`: {why}");
    }
}

/// What is wrong with the planted registry of `checks`, one line each, sorted: a check with no
/// planted case, no failing case or no passing case, and a check whose `planted` holds a case
/// from another file than its own `tests/planted.rs`. It runs no case: the test `cases!` makes
/// for each case does, and judges it more strictly than a pass or a fail.
#[cfg(test)]
pub(crate) fn registry_problems(checks: &[StaticCheck]) -> Vec<String> {
    let mut problems = Vec::new();
    for check in checks {
        let name = check.name;
        if check.planted.is_empty() {
            problems.push(format!("{name} has no planted cases"));
            continue;
        }
        if !check.planted.iter().any(Planted::passes) {
            problems.push(format!("{name} has no planted case that passes"));
        }
        if check.planted.iter().all(Planted::passes) {
            problems.push(format!("{name} has no planted case that fails"));
        }
        let own = own_list(check.source);
        if let Some(case) = check
            .planted
            .iter()
            .find(|case| case.file.replace('\\', "/") != own)
        {
            problems.push(format!(
                "{name}'s planted list holds `{}` from {}; point its CHECK.planted at \
                 tests::planted::PLANTED, the list in {own}",
                case.name,
                case.file.replace('\\', "/")
            ));
        }
    }
    problems.sort();
    problems
}

/// The file that holds the `cases!` list of the check in `source`: `xtask/src/checks/budgets.rs`
/// has `xtask/src/checks/budgets/tests/planted.rs`, with `/` on every OS.
#[cfg(test)]
fn own_list(source: &str) -> String {
    let source = source.replace('\\', "/");
    let stem = source.strip_suffix(".rs").unwrap_or(&source);
    format!("{stem}/tests/planted.rs")
}

/// Makes `PLANTED` and the `planted` test module from one list of cases. See the module docs.
#[cfg(test)]
macro_rules! cases {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident : $expect:ident
            $( ( $($arg:tt)* ) )?
            $( [ $($line:expr),+ $(,)? ] )?
            , inspects $inspects:literal => $repo:path
    ),+ $(,)?) => {
        /// This check's planted cases: repos it must fail on and repos it must pass.
        #[cfg(test)]
        pub(crate) const PLANTED: &[$crate::checks::planted::Planted] = &[$(
            $crate::checks::planted::Planted {
                name: stringify!($name),
                file: file!(),
                expect: $crate::checks::planted::Expect::$expect
                    $( ( $($arg)* ) )?
                    $( (|| vec![$($line),+]) )?,
                inspects: $inspects,
                repo: $repo,
            },
        )+];

        $(
            $(#[doc = $doc])*
            #[test]
            fn $name() {
                $crate::checks::planted::verify_named(&super::super::CHECK, stringify!($name));
            }
        )+
    };
}

#[cfg(test)]
pub(crate) use cases;

#[cfg(test)]
use crate::checks::{Report, StaticCheck};
#[cfg(test)]
use crate::fix::Fix;
