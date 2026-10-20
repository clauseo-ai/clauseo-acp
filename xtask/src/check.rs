//! `cargo xtask check`: works out the scope, plans the steps, runs them, prints the summary.

use crate::changes::{self, ScanRange, Selection};
use crate::output::Printer;
use crate::process::{self, Cmd, Program};
use crate::steps::{self, Scope, StepStatus};
use crate::workspace::{self, Context};
use std::io;
use std::path::Path;
use std::process::ExitCode;

/// Runs the whole check. `quick` limits clippy and nextest to changed packages.
pub(crate) fn run(quick: bool) -> ExitCode {
    let mut out = Printer::stdout();
    let root = workspace::workspace_root();
    let ci = is_ci(std::env::var("CI").ok().as_deref());
    let ctx = Context {
        members: workspace::load(&root),
        root,
        load_resolved_graph: true,
    };
    // Printed once here in full. Each check that stops on it quotes one line and points up.
    if let Err(error) = &ctx.members {
        out.note(format!("note: {error}"));
    }

    let scope = if quick {
        quick_scope(&mut out, &ctx)
    } else {
        Scope::Workspace
    };
    let range = gitleaks_range(&mut out, &ctx, ci);
    let steps = steps::plan(&scope, ci, range.as_deref(), ci || pnpm_found(&ctx.root));
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

/// Whether `pnpm` starts. Outside CI, without it, the ui steps are skipped (see `steps::plan`). CI
/// runs the ui steps whatever this says, so it does not probe there.
fn pnpm_found(root: &Path) -> bool {
    process::run_quiet(&Cmd::new(Program::Pnpm, ["--version"]), root).is_ok()
}

/// CI mode: `CI` is set, is not empty, and is not `0` or `false` in any case.
pub(crate) fn is_ci(value: Option<&str>) -> bool {
    match value {
        None | Some("") => false,
        Some(value) => !(value == "0" || value.eq_ignore_ascii_case("false")),
    }
}

/// Decides what `--quick` covers, and prints the decision.
/// When `cargo metadata` failed, the checks that read members quote its error.
fn quick_scope(out: &mut Printer<io::Stdout>, ctx: &Context) -> Scope {
    let Ok(members) = &ctx.members else {
        out.note(
            "quick: all packages (`cargo metadata` failed, so the changed packages are unknown)",
        );
        return Scope::Workspace;
    };
    let files = match changes::changed_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => {
            out.note(format!(
                "quick: all packages (could not diff against origin/main: {error})"
            ));
            return Scope::Workspace;
        }
    };
    match changes::select(&files, members) {
        Selection::All(reason) => {
            out.note(format!("quick: all packages ({reason})"));
            Scope::Workspace
        }
        Selection::Packages(names) => {
            if names.is_empty() {
                out.note("quick: no changed packages");
            } else {
                let direct = changes::direct_members(&files, members);
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

/// The `--log-opts` range gitleaks scans (see `changes::scan_range`), or `None` for all history.
/// Prints the choice when it is not this branch's own commits.
fn gitleaks_range(out: &mut Printer<io::Stdout>, ctx: &Context, ci: bool) -> Option<String> {
    let range = changes::scan_range(&ctx.root, ci);
    match &range {
        ScanRange::Since(_) => {}
        ScanRange::LastCommit => {
            out.note("gitleaks: the last commit (HEAD is the merge-base with origin/main)");
        }
        ScanRange::AllHistory(why) => out.note(format!("gitleaks: all history ({why})")),
    }
    range.log_opts()
}

fn list_or_none(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
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
