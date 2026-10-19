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
    place: "in budgets.toml at the repo root",
    source: "xtask/src/budgets.rs",
    run,
};

fn run(ctx: &Context) -> Inspection {
    let (rows, violations) = budgets::inspect(&ctx.root);
    Inspection::new(rows, violations)
}

#[cfg(test)]
mod tests;
