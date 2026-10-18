//! Planted violations: inputs a check must fail on (the ways around it included) and inputs it
//! must pass, kept as one list per check so a registry can collect them.
//!
//! Each case is a function that builds its own input (a scratch dir, a scratch workspace or a
//! string) and runs the check on it. `cases!`, written once at the top level of a check's file,
//! turns the list into `PLANTED`, a `&[Planted]`, and into one `#[test]` per case in a module
//! named `planted`, so `cargo test -p xtask planted` lists `checks::<check>::planted::<case>`:
//!
//! ```ignore
//! crate::checks::planted::cases! {
//!     /// A row whose limit is below zero.
//!     negative_limit: FailsWith(&["budgets.toml:3: ...\n    fix: ..."]) => tests::negative_limit,
//!     broken_toml: Fails => tests::broken_toml,
//!     valid_row: Passes => tests::valid_row,
//! }
//! ```
//!
//! The expectation is a variant of `Expect`, written without the type. The functions return an
//! `Inspection` and live in the check's test module, visible to the check's file.

/// What a planted case must produce.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// At least one violation.
    Fails,
    /// Exactly these violations, each as it prints (`path:line: what\n    fix: ...`), in order.
    FailsWith(&'static [&'static str]),
    /// No violation, and at least one item inspected: a pass that inspected nothing proves
    /// nothing.
    Passes,
}

/// One planted case of a check.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Planted {
    /// The case's name, also the name of its test.
    pub name: &'static str,
    pub expect: Expect,
    /// Builds the input and runs the check on it.
    pub run: fn() -> crate::checks::Inspection,
}

#[cfg(test)]
impl Planted {
    /// Runs the case. `Err` says how the result differs from `expect`.
    pub(crate) fn verdict(&self) -> Result<(), String> {
        judge(self.expect, &(self.run)())
    }
}

/// Whether `found` meets `expect`; `Err` says how it does not.
#[cfg(test)]
fn judge(expect: Expect, found: &crate::checks::Inspection) -> Result<(), String> {
    let printed: Vec<String> = found.violations.iter().map(ToString::to_string).collect();
    match expect {
        Expect::Fails if printed.is_empty() => Err(format!(
            "expected at least one violation, found none ({} inspected)",
            found.inspected
        )),
        Expect::FailsWith(expected) if printed != expected => Err(format!(
            "expected these violations:\n{}\nfound:\n{}",
            expected.join("\n"),
            printed.join("\n")
        )),
        Expect::Passes if !printed.is_empty() => Err(format!(
            "expected no violation, found:\n{}",
            printed.join("\n")
        )),
        Expect::Passes if found.inspected == 0 => {
            Err("expected a pass, but the check inspected nothing".to_owned())
        }
        Expect::Fails | Expect::FailsWith(_) | Expect::Passes => Ok(()),
    }
}

/// Runs the case called `name` in `cases` and panics with its verdict when it fails. The body of
/// every test `cases!` makes.
#[cfg(test)]
pub(crate) fn verify_named(cases: &[Planted], name: &str) {
    let Some(case) = cases.iter().find(|case| case.name == name) else {
        panic!("no planted case `{name}`");
    };
    if let Err(why) = case.verdict() {
        panic!("planted case `{name}`: {why}");
    }
}

/// Makes `PLANTED` and the `planted` test module from one list of cases. See the module docs.
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

        /// One test per planted case, named after it.
        #[cfg(test)]
        mod planted {
            $(
                $(#[doc = $doc])*
                #[test]
                fn $name() {
                    $crate::checks::planted::verify_named(super::PLANTED, stringify!($name));
                }
            )+
        }
    };
}

pub(crate) use cases;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::{Inspection, violation};

    fn clean() -> Inspection {
        Inspection::new(1, Vec::new())
    }

    fn empty() -> Inspection {
        Inspection::default()
    }

    fn one_violation() -> Inspection {
        Inspection::new(1, vec![violation("a.toml", 2, "bad", "fix it")])
    }

    fn case(expect: Expect, run: fn() -> Inspection) -> Planted {
        Planted {
            name: "case",
            expect,
            run,
        }
    }

    #[test]
    fn fails_needs_a_violation() {
        assert_eq!(case(Expect::Fails, one_violation).verdict(), Ok(()));
        assert_eq!(
            case(Expect::Fails, clean).verdict(),
            Err("expected at least one violation, found none (1 inspected)".to_owned())
        );
    }

    #[test]
    fn fails_with_needs_the_exact_text() {
        let exact = case(
            Expect::FailsWith(&["a.toml:2: bad\n    fix: fix it"]),
            one_violation,
        );
        assert_eq!(exact.verdict(), Ok(()));
        let other = case(
            Expect::FailsWith(&["a.toml:3: bad\n    fix: fix it"]),
            one_violation,
        );
        assert_eq!(
            other.verdict(),
            Err("expected these violations:\na.toml:3: bad\n    fix: fix it\nfound:\na.toml:2: bad\n    fix: fix it".to_owned())
        );
    }

    #[test]
    fn passes_needs_no_violation_and_something_inspected() {
        assert_eq!(case(Expect::Passes, clean).verdict(), Ok(()));
        assert_eq!(
            case(Expect::Passes, one_violation).verdict(),
            Err("expected no violation, found:\na.toml:2: bad\n    fix: fix it".to_owned())
        );
        assert_eq!(
            case(Expect::Passes, empty).verdict(),
            Err("expected a pass, but the check inspected nothing".to_owned())
        );
    }
}
