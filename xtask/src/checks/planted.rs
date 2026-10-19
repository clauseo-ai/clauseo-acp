//! Planted violations: inputs a check must fail on (the ways around it included) and inputs it
//! must pass, kept as one list per check so a registry can collect them.
//!
//! Every check keeps them in one place, out of its production file:
//!
//! - `<check>/tests/planted.rs` holds the one `cases!` list, declared from `<check>/tests.rs`.
//!   `cases!` turns the list into `PLANTED`, a `&[Planted]`, and into one `#[test]` per case, so
//!   `cargo test -p xtask planted` lists `checks::<check>::tests::planted::<case>`.
//! - The functions that build each case's input live in the check's tests module,
//!   `<check>/tests.rs`, or in `<check>/tests/cases.rs` when they would push `tests.rs` past 500
//!   lines. Never in `planted.rs` itself: each case's test takes its name there.
//!
//! ```ignore
//! crate::checks::planted::cases! {
//!     /// A row whose limit is below zero.
//!     negative_limit: FailsWith(&["budgets.toml:3: ...\n    fix: ..."]) => super::negative_limit,
//!     valid_row: Passes => super::valid_row,
//! }
//! ```
//!
//! The expectation is a variant of `Expect`, written without the type. The functions take no
//! argument and return an `Inspection`.

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
    /// Builds the input and runs the check's code on it.
    pub run: fn() -> Inspection,
}

#[cfg(test)]
impl Planted {
    /// Runs the case and judges it as `check`. `Err` says how the result differs from `expect`.
    pub(crate) fn verdict(&self, check: &StaticCheck) -> Result<(), String> {
        let report = check.judge((self.run)());
        let printed = report.printed();
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

/// Makes `PLANTED` and the `planted` test module from one list of cases. See the module docs.
#[cfg(test)]
macro_rules! cases {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident : $expect:ident $( ( $($arg:tt)* ) )? => $run:path
    ),+ $(,)?) => {
        /// This check's planted cases: inputs it must fail on and inputs it must pass.
        #[cfg(test)]
        pub(crate) const PLANTED: &[$crate::checks::planted::Planted] = &[$(
            $crate::checks::planted::Planted {
                name: stringify!($name),
                expect: $crate::checks::planted::Expect::$expect $( ( $($arg)* ) )?,
                run: $run,
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
use crate::checks::{Inspection, StaticCheck};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::{Noun, violation};

    fn clean() -> Inspection {
        Inspection::new(1, Vec::new())
    }

    fn empty() -> Inspection {
        Inspection::default()
    }

    fn one_violation() -> Inspection {
        Inspection::new(1, vec![violation("a.toml", 2, "bad", "fix it")])
    }

    fn stopped() -> Inspection {
        Inspection::stopped(violation("a.toml", 1, "gone", "restore it"))
    }

    const CHECK: StaticCheck = StaticCheck::fake(
        "fake",
        Noun {
            one: "row",
            many: "rows",
        },
        || "in a.toml".to_owned(),
        "xtask/src/checks/fake.rs",
        |_| Inspection::default(),
    );

    fn verdict(expect: Expect, run: fn() -> Inspection) -> Result<(), String> {
        Planted {
            name: "case",
            expect,
            run,
        }
        .verdict(&CHECK)
    }

    #[test]
    fn fails_with_needs_the_exact_text() {
        assert_eq!(
            verdict(
                Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
                one_violation
            ),
            Ok(())
        );
        assert_eq!(
            verdict(
                Expect::FailsWith(&["a.toml:3: bad\n    fix: fix it"]),
                one_violation
            ),
            Err("expected these failures:\na.toml:3: bad\n    fix: fix it\nfound:\na.toml:2: bad\n    fix: fix it".to_owned())
        );
        assert_eq!(
            verdict(Expect::FailsWith(&[]), clean),
            Err("FailsWith needs at least one failure; a case that passes is Passes".to_owned())
        );
    }

    #[test]
    fn fails_with_includes_the_zero_inspected_failure_judged_by_the_check() {
        assert_eq!(
            verdict(
                Expect::FailsWith(&[
                    "a.toml:1: gone\n    fix: restore it",
                    "0 rows inspected in a.toml\n    fix: the check stopped before it could look; fix the failure above first",
                ]),
                stopped
            ),
            Ok(())
        );
        assert_eq!(
            verdict(
                Expect::FailsWith(&["a.toml:1: gone\n    fix: restore it"]),
                stopped
            ),
            Err("expected these failures:\na.toml:1: gone\n    fix: restore it\nfound:\na.toml:1: gone\n    fix: restore it\n0 rows inspected in a.toml\n    fix: the check stopped before it could look; fix the failure above first".to_owned())
        );
    }

    #[test]
    fn passes_needs_no_violation_and_something_inspected() {
        assert_eq!(verdict(Expect::Passes, clean), Ok(()));
        assert_eq!(
            verdict(Expect::Passes, one_violation),
            Err("expected a pass, found:\na.toml:2: bad\n    fix: fix it".to_owned())
        );
        assert_eq!(
            verdict(Expect::Passes, empty),
            Err("expected a pass, found:\n0 rows inspected in a.toml\n    fix: fake looks for rows in a.toml; restore them, or, if they moved, update the consts xtask/src/checks/fake.rs builds that place from".to_owned())
        );
    }
}
