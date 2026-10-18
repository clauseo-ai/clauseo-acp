//! The `budgets.toml` check: every row must follow the rules in `budgets.rs`.

use crate::budgets;
use crate::checks::StaticCheck;
use crate::violation::Violation;
use crate::workspace::Context;

/// Fails on every problem `budgets::load` finds in `budgets.toml`.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "budgets",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    budgets::load(&ctx.root).err().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::workspace_root;

    #[test]
    fn the_repo_budgets_toml_passes() {
        let ctx = Context {
            root: workspace_root(),
            members: Vec::new(),
        };
        assert!(run(&ctx).is_empty(), "{:?}", run(&ctx));
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
