//! `cargo xtask check`: works out the scope, plans the steps, runs them, prints the summary.

use crate::changes::{self, Selection};
use crate::output::Printer;
use crate::process::{self, Cmd};
use crate::steps::{self, Scope, StepStatus};
use crate::workspace::{self, Context};
use std::io;
use std::process::ExitCode;

/// Runs the whole check. `quick` limits clippy and nextest to changed packages.
pub fn run(quick: bool) -> ExitCode {
    let mut out = Printer::stdout();
    let root = workspace::workspace_root();
    let ci = is_ci(std::env::var("CI").ok().as_deref());
    let (members, members_error) = match workspace::load_members(&root) {
        Ok(members) => (members, None),
        Err(error) => {
            out.note(format!("note: {error}"));
            (Vec::new(), Some(error))
        }
    };
    let ctx = Context { root, members };

    let scope = if quick {
        quick_scope(&mut out, &ctx, members_error.as_deref())
    } else {
        Scope::Workspace
    };
    let steps = steps::plan(&scope, ci);
    let mut exec =
        |cmd: &Cmd, on_output: &mut dyn FnMut(&[u8])| process::capture(cmd, &ctx.root, on_output);
    let results = steps::run_all(steps, &ctx, &mut out, &mut exec);
    out.summary(&results);

    let failed = results
        .iter()
        .any(|result| matches!(result.status, StepStatus::Ran { failed: true, .. }));
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// CI mode: `CI` is set, is not empty, and is not `0` or `false` in any case.
pub fn is_ci(value: Option<&str>) -> bool {
    match value {
        None | Some("") => false,
        Some(value) => !(value == "0" || value.eq_ignore_ascii_case("false")),
    }
}

/// Decides what `--quick` covers, and prints the decision.
fn quick_scope(out: &mut Printer<io::Stdout>, ctx: &Context, members_error: Option<&str>) -> Scope {
    if let Some(error) = members_error {
        out.note(format!(
            "quick: all packages (cargo metadata failed: {error})"
        ));
        return Scope::Workspace;
    }
    let files = match changes::changed_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => {
            out.note(format!(
                "quick: all packages (could not diff against origin/main: {error})"
            ));
            return Scope::Workspace;
        }
    };
    match changes::select(&files, &ctx.members) {
        Selection::All(reason) => {
            out.note(format!("quick: all packages ({reason})"));
            Scope::Workspace
        }
        Selection::Packages(names) => {
            if names.is_empty() {
                out.note("quick: no changed packages");
            } else {
                let direct = changes::direct_members(&files, &ctx.members);
                let dependents: Vec<String> = names
                    .iter()
                    .filter(|name| !direct.contains(name))
                    .cloned()
                    .collect();
                out.note(format!(
                    "quick: packages {} (changed: {}; dependents: {})",
                    names.join(", "),
                    list_or_none(&direct),
                    list_or_none(&dependents),
                ));
            }
            Scope::packages(names)
        }
    }
}

fn list_or_none(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_is_set_non_empty_and_not_a_false_value() {
        assert!(!is_ci(None));
        assert!(!is_ci(Some("")));
        assert!(!is_ci(Some("0")));
        assert!(!is_ci(Some("false")));
        assert!(!is_ci(Some("FALSE")));
        assert!(is_ci(Some("1")));
        assert!(is_ci(Some("true")));
        assert!(is_ci(Some("yes")));
    }
}
