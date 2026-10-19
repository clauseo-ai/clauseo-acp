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

crate::checks::planted::cases! {
    /// A row whose limit is below zero fails at the limit's line.
    negative_limit: FailsWith(&[
        "budgets.toml:4: budget window_visible: limit must be zero or more\n    \
         fix: set limit to zero or more, in the row's unit",
    ]) => tests::negative_limit,
    /// A file that is not TOML fails.
    not_toml: Fails => tests::not_toml,
    /// A row with every required key passes.
    valid_row: Passes => tests::valid_row,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    /// A valid row whose limit line (line 4) is `limit`.
    fn row(limit: &str) -> String {
        format!(
            "[[budget]]\nid = \"window_visible\"\nmetric = \"window visible\"\n{limit}\n\
             unit = \"ms\"\ngate = \"alert\"\nmeasured_by = \"marks\"\nstatus = \"unmeasured\"\n\
             source = \"doc 15\"\n"
        )
    }

    /// Runs the check on a scratch repo whose `budgets.toml` is `text`.
    fn run_on(case: &str, text: &str) -> Inspection {
        let dir = Scratch::new(&format!("planted-budgets-{case}"));
        std::fs::write(dir.join(budgets::FILE), text).expect("write budgets.toml");
        run(&Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
        })
    }

    pub(super) fn negative_limit() -> Inspection {
        run_on("negative-limit", &row("limit = -1.0"))
    }

    pub(super) fn not_toml() -> Inspection {
        run_on("not-toml", "[[budget]\nid = \n")
    }

    pub(super) fn valid_row() -> Inspection {
        run_on("valid-row", &row("limit = 250.0"))
    }

    #[test]
    fn rows_with_problems_still_count() {
        let found = run_on(
            "count",
            &format!("{}\n{}", row("limit = -1.0"), row("limit = 1.0")),
        );
        assert_eq!(found.inspected, 2);
        assert_eq!(found.violations.len(), 2);
    }

    #[test]
    fn the_check_is_registered_under_its_name() {
        assert!(
            crate::checks::ALL
                .iter()
                .any(|check| check.name == "budgets")
        );
    }
}
