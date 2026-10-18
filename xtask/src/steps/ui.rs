//! The frontend steps: `pnpm install` for the `ui` workspace, then the package.json scripts that run
//! tsc, oxlint, oxfmt, vitest and vite build. The scripts are the one source of each command; xtask
//! only appends the flags its parsers need. Each builder says what one tool runs and how its output
//! is read. `plan` decides whether they run or are skipped; the pnpm version is pinned by
//! `packageManager` in `ui/package.json`.

use super::ToolStep;
use super::plan::{parse_none, reproduce};
use crate::process::{Cmd, Program};
use crate::violation::{Finding, Violation};
use crate::workspace::Context;

/// Why the ui steps are skipped on a machine without pnpm. CI never skips them.
pub(super) const NO_PNPM: &str = "pnpm not found; install pnpm 12.14.0 to run the ui steps";

/// The fix when pnpm cannot start: install the pinned version, then the workspace.
const PNPM_FIX: &str = "install pnpm 12.14.0 (the `packageManager` in ui/package.json), then run `pnpm -C ui install --frozen-lockfile`";

/// The six ui steps in run order: the install, then the four checks and the two builds.
pub(super) fn steps() -> Vec<ToolStep> {
    vec![
        install_step(),
        tsc_step(),
        oxlint_step(),
        oxfmt_step(),
        vitest_step(),
        vite_build_step(),
    ]
}

/// `pnpm -C <dir> run <script> <args>`: a script that the package.json in `dir` defines. pnpm passes
/// the trailing args to the script's command.
fn script(dir: &str, name: &str, args: &[&str]) -> Cmd {
    Cmd::new(
        Program::Pnpm,
        ["-C", dir, "run", name]
            .into_iter()
            .chain(args.iter().copied()),
    )
}

fn install_step() -> ToolStep {
    ToolStep {
        name: "pnpm-install",
        cmd: Cmd::new(Program::Pnpm, ["-C", "ui", "install", "--frozen-lockfile"]),
        parse: parse_none,
        fix: ui_fix,
        render: None,
    }
}

fn tsc_step() -> ToolStep {
    ToolStep {
        name: "tsc",
        cmd: script("ui/app", "typecheck", &["--pretty", "false"]),
        parse: tsc_findings,
        fix: ui_fix,
        render: None,
    }
}

fn oxlint_step() -> ToolStep {
    ToolStep {
        name: "oxlint",
        cmd: script("ui/app", "lint", &["--format", "unix"]),
        parse: oxlint_findings,
        fix: ui_fix,
        render: None,
    }
}

fn oxfmt_step() -> ToolStep {
    ToolStep {
        name: "oxfmt",
        cmd: script("ui", "fmt:check", &[]),
        parse: oxfmt_findings,
        fix: ui_fix,
        render: None,
    }
}

fn vitest_step() -> ToolStep {
    ToolStep {
        name: "vitest",
        cmd: script("ui/app", "test", &[]),
        parse: parse_none,
        fix: ui_fix,
        render: None,
    }
}

fn vite_build_step() -> ToolStep {
    ToolStep {
        name: "vite-build",
        cmd: script("ui/app", "build", &[]),
        parse: parse_none,
        fix: ui_fix,
        render: None,
    }
}

/// The fix for a ui step with no finding: pnpm could not start (`None`), or the command reproduces
/// the failure (`Some`, the captured output).
fn ui_fix(cmd: &Cmd, output: Option<&str>) -> String {
    match output {
        None => PNPM_FIX.into(),
        Some(_) => reproduce(cmd, output),
    }
}

/// Repo-relative form of a path a ui tool printed. The tool ran in `base` (`ui/app` for tsc and
/// oxlint, `ui` for oxfmt), so its paths are relative to that directory.
fn ui_path(base: &str, raw: &str) -> String {
    let path = raw.trim().replace('\\', "/");
    format!(
        "{base}/{}",
        path.strip_prefix("./").unwrap_or(path.as_str())
    )
}

/// The `path(line,col): error TSnnnn: message` lines from `tsc --noEmit --pretty false`. Every
/// other line (tsc's summary, a config error without a location) is ignored.
fn tsc_findings(output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    output.lines().filter_map(tsc_finding).collect()
}

fn tsc_finding(line: &str) -> Option<Finding> {
    let (location, message) = line.split_once("): ")?;
    let (path, position) = location.rsplit_once('(')?;
    let line_no: u32 = position.split_once(',')?.0.parse().ok()?;
    if path.is_empty() {
        return None;
    }
    Some(Finding {
        violation: Violation {
            path: ui_path("ui/app", path),
            line: line_no,
            message: message.to_owned(),
            fix: "`pnpm -C ui/app run typecheck` shows the full error".into(),
        },
        is_error: message.starts_with("error "),
    })
}

/// The `path:line:col: message [Level/rule]` lines from `oxlint --format unix`. Only an `Error/`
/// rule fails the step; a `Warning/` rule is printed and counted. The `N problems` summary is
/// ignored.
fn oxlint_findings(output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    output.lines().filter_map(oxlint_finding).collect()
}

fn oxlint_finding(line: &str) -> Option<Finding> {
    let mut parts = line.splitn(4, ':');
    let path = parts.next()?;
    let line_no: u32 = parts.next()?.parse().ok()?;
    parts.next()?.parse::<u32>().ok()?;
    let message = parts.next()?.trim();
    if path.is_empty() || !message.ends_with(']') {
        return None;
    }
    let is_error = message
        .rsplit_once('[')
        .is_some_and(|(_, rule)| rule.starts_with("Error/"));
    Some(Finding {
        violation: Violation {
            path: ui_path("ui/app", path),
            line: line_no,
            message: message.to_owned(),
            fix: "`pnpm -C ui/app run lint` shows the rule's help; fix the code, do not disable the rule"
                .into(),
        },
        is_error,
    })
}

/// One finding per file that `oxfmt --check` would reformat. Its lines name each such file as
/// `path (0ms)`; the clean run prints no such line.
fn oxfmt_findings(output: &str, _ctx: &Context, _cmd: &Cmd) -> Vec<Finding> {
    output
        .lines()
        .filter_map(oxfmt_path)
        .map(|path| Finding {
            violation: Violation {
                path: ui_path("ui", path),
                line: 1,
                message: "oxfmt would reformat this file".into(),
                fix: "run `pnpm -C ui run fmt` to reformat it".into(),
            },
            is_error: true,
        })
        .collect()
}

/// The path in a `path (<n>ms)` line, or `None` for any other line.
fn oxfmt_path(line: &str) -> Option<&str> {
    let (path, timing) = line.rsplit_once(" (")?;
    let millis = timing.strip_suffix("ms)")?;
    let numeric = !millis.is_empty() && millis.chars().all(|c| c.is_ascii_digit() || c == '.');
    (numeric && !path.is_empty()).then_some(path)
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn context() -> Context {
        Context {
            root: PathBuf::from("/ws"),
            members: Vec::new(),
        }
    }

    /// A finding as `(path, line, message, is_error)`.
    type Row = (String, u32, String, bool);

    fn findings(
        parse: fn(&str, &Context, &Cmd) -> Vec<Finding>,
        cmd: &Cmd,
        output: &str,
    ) -> Vec<Row> {
        parse(output, &context(), cmd)
            .into_iter()
            .map(|f| {
                (
                    f.violation.path,
                    f.violation.line,
                    f.violation.message,
                    f.is_error,
                )
            })
            .collect()
    }

    fn row(path: &str, line: u32, message: &str, is_error: bool) -> Row {
        (path.into(), line, message.into(), is_error)
    }

    #[test]
    fn commands_are_the_package_scripts_with_the_parse_flags() {
        assert_eq!(
            install_step().cmd.to_string(),
            "pnpm -C ui install --frozen-lockfile"
        );
        assert_eq!(
            tsc_step().cmd.to_string(),
            "pnpm -C ui/app run typecheck --pretty false"
        );
        assert_eq!(
            oxlint_step().cmd.to_string(),
            "pnpm -C ui/app run lint --format unix"
        );
        assert_eq!(oxfmt_step().cmd.to_string(), "pnpm -C ui run fmt:check");
        assert_eq!(vitest_step().cmd.to_string(), "pnpm -C ui/app run test");
        assert_eq!(
            vite_build_step().cmd.to_string(),
            "pnpm -C ui/app run build"
        );
    }

    #[test]
    fn a_pnpm_that_cannot_start_gets_the_install_fix_and_others_reproduce() {
        let cmd = vitest_step().cmd;
        assert_eq!(
            ui_fix(&cmd, None),
            "install pnpm 12.14.0 (the `packageManager` in ui/package.json), then run `pnpm -C ui install --frozen-lockfile`"
        );
        assert_eq!(
            ui_fix(&cmd, Some("FAIL  src/App.test.tsx")),
            "run `pnpm -C ui/app run test` to reproduce"
        );
    }

    /// Real `pnpm -C ui/app run typecheck --pretty false` output with two errors planted in
    /// `src/zz_planted.tsx`: pnpm's echo of the script and its `[ELIFECYCLE]` line are in it.
    const TSC_FAILING: &str = "$ tsc --noEmit --pretty false\n\
        src/zz_planted.tsx(1,24): error TS2307: Cannot find module '@tauri-apps/api/core' or its corresponding type declarations.\n\
        src/zz_planted.tsx(2,7): error TS2322: Type 'string' is not assignable to type 'number'.\n\
        [ELIFECYCLE] Command failed with exit code 1.\n";

    #[test]
    fn tsc_turns_each_error_line_into_a_finding_under_ui_app() {
        let cmd = tsc_step().cmd;
        assert_eq!(
            findings(tsc_findings, &cmd, TSC_FAILING),
            [
                row(
                    "ui/app/src/zz_planted.tsx",
                    1,
                    "error TS2307: Cannot find module '@tauri-apps/api/core' or its corresponding type declarations.",
                    true,
                ),
                row(
                    "ui/app/src/zz_planted.tsx",
                    2,
                    "error TS2322: Type 'string' is not assignable to type 'number'.",
                    true,
                ),
            ]
        );
        assert_eq!(
            tsc_findings(TSC_FAILING, &context(), &cmd)[0].violation.fix,
            "`pnpm -C ui/app run typecheck` shows the full error"
        );
    }

    #[test]
    fn tsc_finds_nothing_in_a_clean_run() {
        // Real output of a passing run: pnpm echoes the script, tsc prints nothing.
        assert_eq!(
            findings(
                tsc_findings,
                &tsc_step().cmd,
                "$ tsc --noEmit --pretty false\n"
            ),
            Vec::<Row>::new()
        );
    }

    /// Real `pnpm -C ui/app run lint --format unix` output with an error planted in
    /// `src/zz_planted.tsx`.
    const OXLINT_FAILING: &str = "$ oxlint --format unix\n\
        src/zz_planted.tsx:1:1: '@tauri-apps/api/core' import is restricted from being used by a pattern. [Error/eslint(no-restricted-imports)]\n\
        \n\
        1 problem\n\
        [ELIFECYCLE] Command failed with exit code 1.\n";

    /// The same kind of line when `no-unused-vars` is only a warning (`oxlint -W no-unused-vars`).
    const OXLINT_WARNING: &str = "src/zz_fmt2.ts:1:9: Variable 'b' is declared but never used. Unused variables should start with a '_'. [Warning/eslint(no-unused-vars)]\n\
        \n\
        1 problem\n";

    #[test]
    fn oxlint_error_lines_fail_and_name_the_rule_in_the_message() {
        let cmd = oxlint_step().cmd;
        assert_eq!(
            findings(oxlint_findings, &cmd, OXLINT_FAILING),
            [row(
                "ui/app/src/zz_planted.tsx",
                1,
                "'@tauri-apps/api/core' import is restricted from being used by a pattern. [Error/eslint(no-restricted-imports)]",
                true,
            )]
        );
    }

    #[test]
    fn oxlint_warning_lines_are_findings_that_do_not_fail() {
        let found = findings(oxlint_findings, &oxlint_step().cmd, OXLINT_WARNING);
        assert_eq!(found.len(), 1);
        assert!(!found[0].3, "a Warning/ rule must not fail the step");
    }

    #[test]
    fn oxlint_finds_nothing_in_a_clean_run() {
        // Real output of a passing run: pnpm echoes the script, oxlint prints nothing.
        assert_eq!(
            findings(
                oxlint_findings,
                &oxlint_step().cmd,
                "$ oxlint --format unix\n"
            ),
            Vec::<Row>::new()
        );
    }

    /// Real `pnpm -C ui run fmt:check` output with two files planted unformatted in `ui/app/src`.
    /// oxfmt names them relative to `ui/`.
    const OXFMT_FAILING: &str = "$ oxfmt --check\n\
        Checking formatting...\n\
        \n\
        app/src/zz_fmt.ts (0ms)\n\
        app/src/zz_planted.tsx (0ms)\n\
        \n\
        Format issues found in above 2 files. Run without `--check` to fix.\n\
        Finished in 124ms on 16 files using 12 threads.\n\
        [ELIFECYCLE] Command failed with exit code 1.\n";

    /// Real `pnpm -C ui run fmt:check` output for a clean tree.
    const OXFMT_CLEAN: &str = "$ oxfmt --check\n\
        Checking formatting...\n\
        \n\
        All matched files use the correct format.\n\
        Finished in 146ms on 14 files using 12 threads.\n";

    #[test]
    fn oxfmt_names_each_unformatted_file_at_line_one() {
        let cmd = oxfmt_step().cmd;
        assert_eq!(
            findings(oxfmt_findings, &cmd, OXFMT_FAILING),
            [
                row(
                    "ui/app/src/zz_fmt.ts",
                    1,
                    "oxfmt would reformat this file",
                    true
                ),
                row(
                    "ui/app/src/zz_planted.tsx",
                    1,
                    "oxfmt would reformat this file",
                    true
                ),
            ]
        );
        assert_eq!(
            oxfmt_findings(OXFMT_FAILING, &context(), &cmd)[0]
                .violation
                .fix,
            "run `pnpm -C ui run fmt` to reformat it"
        );
    }

    #[test]
    fn oxfmt_names_a_file_at_the_ui_root_under_ui() {
        // oxfmt runs in `ui/`, so `ui/package.json` is printed as `package.json`.
        assert_eq!(
            findings(oxfmt_findings, &oxfmt_step().cmd, "package.json (0ms)\n"),
            [row(
                "ui/package.json",
                1,
                "oxfmt would reformat this file",
                true
            )]
        );
    }

    #[test]
    fn oxfmt_finds_nothing_in_a_clean_run() {
        assert_eq!(
            findings(oxfmt_findings, &oxfmt_step().cmd, OXFMT_CLEAN),
            Vec::<Row>::new()
        );
    }

    #[test]
    fn oxfmt_path_needs_the_timing_suffix() {
        assert_eq!(oxfmt_path("src/a b.ts (12ms)"), Some("src/a b.ts"));
        assert_eq!(oxfmt_path("Finished in 148ms on 13 files"), None);
        assert_eq!(oxfmt_path("src/a.ts (slow)"), None);
    }
}
