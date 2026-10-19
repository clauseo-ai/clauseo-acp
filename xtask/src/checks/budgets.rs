//! The `budgets.toml` check: every row must follow the rules in `budgets.rs`. It counts the
//! `[[budget]]` rows it read, with or without problems.

use crate::budgets;
use crate::checks::{Inspection, Noun, StaticCheck};
use crate::workspace::Context;

/// Fails on every problem `budgets::inspect` finds in `budgets.toml`.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "budgets",
    counts: Noun {
        one: "budget row",
        many: "budget rows",
    },
    place: || format!("in {} at the repo root", budgets::FILE),
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// A file that is there but cannot be read or parsed stops the check: no row was looked at.
fn run(ctx: &Context) -> Inspection {
    let read = budgets::inspect(&ctx.root);
    if read.unreadable {
        Inspection::stopped_on(read.violations)
    } else {
        Inspection::new(read.rows, read.violations)
    }
}

#[cfg(test)]
mod tests;
