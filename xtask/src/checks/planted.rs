//! Planted violations: scratch repos a check must fail on (the ways around it included) and repos
//! it must pass, kept as one list per check, `StaticCheck::planted`, which is the registry.
//!
//! Every check keeps them in one place, out of its production file:
//!
//! - `<check>/tests/planted.rs` holds the one `cases!` list, declared from `<check>/tests.rs`.
//!   `cases!` turns the list into `PLANTED`, a `&[Planted]`, and into one `#[test]` per case, so
//!   `cargo test -p xtask planted` lists `checks::<check>::tests::planted::<case>`.
//! - The functions that build each case's repo live in the check's tests module,
//!   `<check>/tests.rs`, or in `<check>/tests/cases.rs` when they would push `tests.rs` past 500
//!   lines. Never in `planted.rs` itself: each case's test takes its name there.
//!
//! ```ignore
//! crate::checks::planted::cases! {
//!     /// A row whose limit is below zero: the one row is read, and it fails.
//!     negative_limit: FailsWith(&["budgets.toml:3: ...\n    fix: ..."]),
//!         inspects 1 => super::negative_limit,
//!     valid_row: Passes, inspects 1 => super::valid_row,
//! }
//! ```
//!
//! The expectation is a variant of `Expect`, written without the type. Every case also states
//! `inspects N`: how many items its check must report inspecting (`Inspection::inspected`). N is
//! worked out from what the case's builder writes (the files, rows or crates the check reads),
//! not copied from a run, so a check that counts what it never read fails its cases.
//!
//! A builder takes no argument and returns a `Repo`: a scratch directory holding the case's one
//! violation (or none) and the `Context` the check runs in. It never runs the check. The harness
//! does, through `StaticCheck::report`, the same call `cargo xtask check` makes, so no case can
//! reach an inner function and skip what the whole check does: what it reads, how it counts and
//! its zero-inspected judging. The registry test in `tests.rs` runs every check in `ALL` on each
//! of its repos.

#[cfg(test)]
mod repo;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use repo::{Repo, git};

/// What a planted case must produce, judged by the check's own `StaticCheck::judge`.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// Exactly these violations and failures, each as it prints (`path:line: what\n    fix: ...`,
    /// or `what\n    fix: ...` for one without a location), in the order the runner prints them.
    /// The zero-inspected failure is one of them when the case inspected nothing.
    FailsWith(&'static [&'static str]),
    /// The check passes: no violation, no failure, and at least one item inspected.
    Passes,
}

/// One planted case of a check.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Planted {
    /// The case's name, also the name of its test.
    pub name: &'static str,
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
        match self.expect {
            Expect::FailsWith([]) => {
                Err("FailsWith needs at least one failure; a case that passes is Passes".to_owned())
            }
            Expect::FailsWith(expected) if printed != expected => Err(format!(
                "expected these failures:\n{}\nfound:\n{}",
                expected.join("\n"),
                printed.join("\n")
            )),
            Expect::Passes if report.failed() => {
                Err(format!("expected a pass, found:\n{}", printed.join("\n")))
            }
            Expect::FailsWith(_) | Expect::Passes => Ok(()),
        }
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

/// What is wrong with the planted registry of `checks`, one line each: a check with no planted
/// case, no failing case or no passing case, a failing case the check passes, and a passing case
/// it fails. Every case runs through `StaticCheck::report`, on as many threads as the machine
/// has, since a case that runs cargo takes a tenth of a second.
#[cfg(test)]
pub(crate) fn registry_problems(checks: &[StaticCheck]) -> Vec<String> {
    let mut problems = Vec::new();
    for check in checks {
        let name = check.name;
        if check.planted.is_empty() {
            problems.push(format!("{name} has no planted cases"));
            continue;
        }
        if !check
            .planted
            .iter()
            .any(|case| case.expect == Expect::Passes)
        {
            problems.push(format!("{name} has no planted case that passes"));
        }
        if !check
            .planted
            .iter()
            .any(|case| case.expect != Expect::Passes)
        {
            problems.push(format!("{name} has no planted case that fails"));
        }
    }
    let cases: Vec<(&StaticCheck, &Planted)> = checks
        .iter()
        .flat_map(|check| check.planted.iter().map(move |case| (check, case)))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    // Each thread takes the next case no thread has taken, so the slow cases (those that run
    // cargo, listed together under their check) spread over every thread instead of one.
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        // Every thread starts before the first join, so the cases run side by side.
        let mut handles = Vec::new();
        for _ in 0..threads.min(cases.len()) {
            let (cases, next) = (&cases, &next);
            handles.push(scope.spawn(move || {
                let mut found = Vec::new();
                while let Some((check, case)) = cases.get(next.fetch_add(1, Ordering::Relaxed)) {
                    found.extend(judged(check, case));
                }
                found
            }));
        }
        for handle in handles {
            let found = handle
                .join()
                .expect("a planted case panicked; its own test says why");
            problems.extend(found);
        }
    });
    problems
}

/// The registry's problem with `case` of `check`: a failing case with no violation or failure,
/// or a passing case with one.
#[cfg(test)]
fn judged(check: &StaticCheck, case: &Planted) -> Option<String> {
    let report = case.report(check);
    match (case.expect, report.failed()) {
        (Expect::FailsWith(_), false) => Some(format!(
            "{} found nothing in its planted case `{}`, which holds a violation",
            check.name, case.name
        )),
        (Expect::Passes, true) => Some(format!(
            "{} failed its planted case `{}`, which holds none:\n{}",
            check.name,
            case.name,
            report.printed().join("\n")
        )),
        _ => None,
    }
}

/// Makes `PLANTED` and the `planted` test module from one list of cases. See the module docs.
#[cfg(test)]
macro_rules! cases {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident : $expect:ident $( ( $($arg:tt)* ) )? , inspects $inspects:literal => $repo:path
    ),+ $(,)?) => {
        /// This check's planted cases: repos it must fail on and repos it must pass.
        #[cfg(test)]
        pub(crate) const PLANTED: &[$crate::checks::planted::Planted] = &[$(
            $crate::checks::planted::Planted {
                name: stringify!($name),
                expect: $crate::checks::planted::Expect::$expect $( ( $($arg)* ) )?,
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
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
