//! `cargo xtask check`: works out the scope, plans the steps, runs them, prints the summary.

use crate::changes::{self, ScanRange, Selection};
use crate::output::Printer;
use crate::process::{self, Cmd, Program, QuietRunner};
use crate::steps::{self, Exec, Scope, StepStatus};
use crate::workspace::Context;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// Runs the whole check on `ctx`: `quick` limits clippy and nextest to changed packages, `ci` is
/// the CI mode, every tool command goes through `exec` and the pnpm probe through `quiet`.
/// `main.rs` passes the real runners, `process::capture` and `process::run_quiet`.
pub(crate) fn run(
    ctx: &Context,
    quick: bool,
    ci: bool,
    exec: &mut Exec<'_>,
    quiet: &mut QuietRunner<'_>,
) -> ExitCode {
    let mut out = Printer::stdout();
    // Printed once here in full. Each check that stops on it quotes one line and points up.
    if let Err(error) = &ctx.members {
        out.note(format!("note: {error}"));
    }

    let scope = if quick {
        quick_scope(&mut out, ctx, changes::changed_files(&ctx.root))
    } else {
        Scope::Workspace
    };
    let range = gitleaks_range(&mut out, &changes::scan_range(&ctx.root, ci));
    let steps = steps::plan(
        &scope,
        ci,
        range.as_deref(),
        ci || pnpm_found(&ctx.root, quiet),
    );
    let results = steps::run_all(steps, ctx, &mut out, exec);
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

/// How long `pnpm --version` may run: it prints one line, so 30 s means it hangs.
const PNPM_VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether `pnpm` starts. Outside CI, without it, the ui steps are skipped (see `steps::plan`). CI
/// runs the ui steps whatever this says, so it does not probe there. A pnpm that starts but does
/// not finish is found: the ui steps then run and report it. `runner` runs `pnpm --version`.
fn pnpm_found(root: &Path, runner: &mut QuietRunner<'_>) -> bool {
    let cmd = Cmd::new(Program::Pnpm, ["--version"], PNPM_VERSION_TIMEOUT);
    match runner(&cmd, root) {
        Ok(_) => true,
        Err(error) => process::is_timeout(&error),
    }
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
fn quick_scope<W: Write>(
    out: &mut Printer<W>,
    ctx: &Context,
    changed: Result<Vec<String>, String>,
) -> Scope {
    let Ok(members) = &ctx.members else {
        out.note(
            "quick: all packages (`cargo metadata` failed, so the changed packages are unknown)",
        );
        return Scope::Workspace;
    };
    let files = match changed {
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
fn gitleaks_range<W: Write>(out: &mut Printer<W>, range: &ScanRange) -> Option<String> {
    match range {
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
    use crate::scratch::Scratch;
    use crate::workspace::{Context, Member, PackageDependencies};
    use std::cell::RefCell;
    use std::io;

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

    /// A checkout in a scratch dir with no workspace members, so `run` loads no metadata and the
    /// graph stays off, as in the tests of the real repo.
    fn scratch_context(name: &str) -> (Scratch, Context) {
        let root = Scratch::new(name);
        let ctx = Context {
            root: root.to_path_buf(),
            members: Ok(Vec::new()),
            load_resolved_graph: false,
        };
        (root, ctx)
    }

    /// Records `cmd` in `ran` and fails it: no tool runs.
    fn record_and_fail(ran: &RefCell<Vec<String>>, cmd: &Cmd) -> process::Captured {
        ran.borrow_mut().push(cmd.to_string());
        process::Captured {
            success: false,
            output: "fake failure\n".into(),
        }
    }

    #[test]
    fn a_check_whose_commands_fail_exits_with_failure() {
        let (_root, ctx) = scratch_context("check-run");
        let ran = RefCell::new(Vec::new());
        let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| Ok(record_and_fail(&ran, cmd));
        let mut quiet = |_: &Cmd, _: &Path| -> io::Result<process::Quiet> {
            Ok(process::Quiet {
                success: true,
                stdout: "9.15.0\n".into(),
                stderr: String::new(),
            })
        };
        assert_eq!(
            run(&ctx, false, false, &mut exec, &mut quiet),
            ExitCode::FAILURE
        );
        assert!(ran.borrow().iter().any(|cmd| cmd.starts_with("cargo fmt")));
    }

    #[test]
    fn a_local_check_runs_the_ui_steps_when_pnpm_starts() {
        let (_root, ctx) = scratch_context("check-run-pnpm");
        let ran = RefCell::new(Vec::new());
        let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| Ok(record_and_fail(&ran, cmd));
        let mut quiet = |_: &Cmd, _: &Path| -> io::Result<process::Quiet> {
            Ok(process::Quiet {
                success: true,
                stdout: "9.15.0\n".into(),
                stderr: String::new(),
            })
        };
        run(&ctx, false, false, &mut exec, &mut quiet);
        assert!(ran.borrow().iter().any(|cmd| cmd.starts_with("pnpm")));
    }

    #[test]
    fn a_local_check_skips_the_ui_steps_when_pnpm_does_not_start() {
        let (_root, ctx) = scratch_context("check-run-no-pnpm");
        let ran = RefCell::new(Vec::new());
        let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| Ok(record_and_fail(&ran, cmd));
        let mut quiet = |_: &Cmd, _: &Path| -> io::Result<process::Quiet> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        };
        run(&ctx, false, false, &mut exec, &mut quiet);
        assert!(!ran.borrow().iter().any(|cmd| cmd.starts_with("pnpm")));
    }

    #[test]
    fn a_ci_check_runs_the_ui_steps_without_probing_for_pnpm() {
        let (_root, ctx) = scratch_context("check-run-ci");
        let ran = RefCell::new(Vec::new());
        let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| Ok(record_and_fail(&ran, cmd));
        let probes = RefCell::new(0);
        let mut quiet = |_: &Cmd, _: &Path| -> io::Result<process::Quiet> {
            *probes.borrow_mut() += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        };
        run(&ctx, false, true, &mut exec, &mut quiet);
        assert_eq!(*probes.borrow(), 0);
        assert!(ran.borrow().iter().any(|cmd| cmd.starts_with("pnpm")));
    }

    /// A member at `dir` that depends on the members whose directories are `path_deps`.
    fn member(name: &str, dir: &str, path_deps: &[&str]) -> Member {
        Member {
            name: name.into(),
            dir: dir.into(),
            path_deps: path_deps.iter().map(|dep| (*dep).to_owned()).collect(),
            targets: Vec::new(),
            dependencies: PackageDependencies::default(),
        }
    }

    /// What a test's printer wrote, as text.
    fn printed(out: Printer<Vec<u8>>) -> String {
        String::from_utf8(out.into_inner()).expect("utf-8 output")
    }

    /// Two members: `app` depends on `core`.
    fn core_and_app() -> Vec<Member> {
        vec![
            member("core", "crates/core", &[]),
            member("app", "crates/app", &["crates/core"]),
        ]
    }

    #[test]
    fn a_quick_check_names_the_changed_packages_and_the_dependents_they_pull_in() {
        let (_root, mut ctx) = scratch_context("check-quick-dependents");
        ctx.members = Ok(core_and_app());
        let mut out = Printer::new(Vec::<u8>::new(), false);
        quick_scope(&mut out, &ctx, Ok(vec!["crates/core/src/lib.rs".into()]));
        assert_eq!(
            printed(out),
            "quick: packages app, core (changed: core; dependents: app)\n"
        );
    }

    #[test]
    fn a_quick_check_with_no_dependents_says_none() {
        let (_root, mut ctx) = scratch_context("check-quick-none");
        ctx.members = Ok(core_and_app());
        let mut out = Printer::new(Vec::<u8>::new(), false);
        quick_scope(&mut out, &ctx, Ok(vec!["crates/app/src/main.rs".into()]));
        assert_eq!(
            printed(out),
            "quick: packages app (changed: app; dependents: none)\n"
        );
    }

    #[test]
    fn gitleaks_scans_the_commits_since_the_merge_base_without_a_note() {
        let mut out = Printer::new(Vec::<u8>::new(), false);
        let range = gitleaks_range(&mut out, &ScanRange::Since("abc123".into()));
        assert_eq!(range, Some("abc123..HEAD".to_owned()));
        assert_eq!(printed(out), "");
    }

    #[test]
    fn gitleaks_scans_the_last_commit_and_says_so() {
        let mut out = Printer::new(Vec::<u8>::new(), false);
        let range = gitleaks_range(&mut out, &ScanRange::LastCommit);
        assert_eq!(range, Some("HEAD~1..HEAD".to_owned()));
        assert_eq!(
            printed(out),
            "gitleaks: the last commit (HEAD is the merge-base with origin/main)\n"
        );
    }

    #[test]
    fn gitleaks_scans_all_history_without_a_range_and_says_why() {
        let mut out = Printer::new(Vec::<u8>::new(), false);
        let range = gitleaks_range(&mut out, &ScanRange::AllHistory("no merge-base".into()));
        assert_eq!(range, None);
        assert_eq!(printed(out), "gitleaks: all history (no merge-base)\n");
    }

    #[test]
    fn pnpm_is_found_when_it_starts_and_prints_its_version() {
        let mut runner = |cmd: &Cmd, _: &Path| {
            assert_eq!(cmd.to_string(), "pnpm --version");
            Ok(process::Quiet {
                success: true,
                stdout: "9.15.0\n".into(),
                stderr: String::new(),
            })
        };
        assert!(pnpm_found(Path::new("."), &mut runner));
    }

    #[test]
    fn pnpm_is_found_when_it_runs_past_its_limit() {
        let mut runner = |cmd: &Cmd, _: &Path| Err(process::timed_out(cmd));
        assert!(pnpm_found(Path::new("."), &mut runner));
    }

    #[test]
    fn pnpm_is_not_found_when_it_cannot_start() {
        let mut runner = |_: &Cmd, _: &Path| Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(!pnpm_found(Path::new("."), &mut runner));
    }
}
