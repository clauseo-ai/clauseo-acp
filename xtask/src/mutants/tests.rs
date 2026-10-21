//! Tests of `cargo xtask mutants`, through a fake `exec`: no real cargo-mutants runs.

use super::*;
use crate::scratch::Scratch;

mod cap;
mod errors;
mod run;
mod untracked;

/// The command that lists the untracked `.rs` files, as it prints.
const UNTRACKED_CMD: &str = "git ls-files -z --others --exclude-standard -- *.rs";

const CMD: &str = "cargo mutants --workspace --in-diff target/xtask/mutants.diff --output \
                   target/xtask --jobs 4";
const MISSED_FIX: &str = "    fix: add a test that fails on this change, or exclude it with a \
                          reason in .cargo/mutants.toml (the code owner approves it)";
const DIFF_TEXT: &str = "diff --git a/xtask/src/main.rs b/xtask/src/main.rs\n";

/// The baseline outcome, with `summary`.
fn baseline(summary: &str) -> String {
    format!(
        r#"{{"scenario": "Baseline", "summary": "{summary}", "log_path": "log/baseline.log",
            "diff_path": null, "phase_results": []}}"#
    )
}

/// A mutant outcome as cargo-mutants 27.1.0 writes it (trimmed of `phase_results`).
fn mutant(summary: &str, line: u32, column: u32, mutation: &str) -> String {
    format!(
        r#"{{"scenario": {{"Mutant": {{
              "name": "xtask/src/main.rs:{line}:{column}: {mutation}", "package": "xtask",
              "file": "xtask/src/main.rs",
              "function": {{"function_name": "double", "return_type": "-> u32",
                "span": {{"start": {{"line": 105, "column": 1}}, "end": {{"line": 107, "column": 2}}}}}},
              "span": {{"start": {{"line": {line}, "column": {column}}},
                        "end": {{"line": {line}, "column": 10}}}},
              "replacement": "0", "genre": "FnValue"}}}},
            "summary": "{summary}", "log_path": "log/main_{line}.log",
            "diff_path": "diff/main_{line}.diff", "phase_results": []}}"#
    )
}

/// An `outcomes.json` with `outcomes`, finished when `finished`.
fn lab(outcomes: &[String], finished: bool) -> String {
    let end = if finished {
        r#""2026-10-20T19:44:09.183114Z""#
    } else {
        "null"
    };
    format!(
        r#"{{"outcomes": [{}], "total_mutants": 0, "missed": 0, "caught": 0, "timeout": 0,
            "unviable": 0, "success": 0, "start_time": "2026-10-20T19:43:18.287389Z",
            "end_time": {end}, "cargo_mutants_version": "27.1.0"}}"#,
        outcomes.join(",")
    )
}

/// What the fake cargo-mutants run does.
enum Run {
    /// Writes this `outcomes.json`, then exits with `success`.
    Writes(String, bool),
    /// Writes nothing and exits with `success`, printing `output`.
    Nothing(bool, &'static str),
    /// Runs past its limit.
    TimesOut,
    /// Writes this `outcomes.json`, then runs past its limit.
    WritesThenTimesOut(String),
    /// Does not start.
    CannotStart,
    /// Leaves a folder where `outcomes.json` should be, and exits 0.
    FolderForOutcomes,
}

/// What the fake `git ls-files` of the untracked `.rs` files does.
enum Git {
    /// Exits 0, printing this NUL-separated list.
    Lists(&'static str),
    /// Exits non-zero, printing this.
    Fails(&'static str),
    /// Runs past its limit.
    TimesOut,
    /// Does not start.
    CannotStart,
}

/// `check_with`, where git lists no untracked `.rs` file.
fn check(
    name: &str,
    diff: Result<String, GitError>,
    version: Option<&str>,
    run: &Run,
) -> (String, bool, Vec<String>) {
    check_with(name, diff, &Git::Lists(""), version, run)
}

/// Runs `run_with` in a fresh scratch repo named `name`, with `diff`, a `git ls-files` that does
/// `git`, a `cargo mutants --version` that prints `version` (`None`: it fails), and a run that
/// does `run`. Returns what it printed, whether it passed, and the commands it ran.
fn check_with(
    name: &str,
    diff: Result<String, GitError>,
    git: &Git,
    version: Option<&str>,
    run: &Run,
) -> (String, bool, Vec<String>) {
    let root = Scratch::new(name);
    let temp = root.join("tmp");
    let prefix = build_dir_prefix(&root);
    // A build dir of an earlier run, which is not this run's to remove.
    let old_build_dir = temp.join(format!("{prefix}old.tmp"));
    Scratch::create_dir_all(&old_build_dir).expect("make an old build dir");
    let new_build_dir = temp.join(format!("{prefix}new.tmp"));
    // A build dir another tree's run made meanwhile, which is not this run's either.
    let other_build_dir = temp.join("cargo-mutants-other-tree-new.tmp");
    let mut ran = Vec::new();
    let mut out = Printer::new(Vec::new(), false);
    let passed = {
        let mut exec = |cmd: &Cmd, _: &mut dyn FnMut(&[u8])| -> io::Result<Ran> {
            ran.push(cmd.to_string());
            if cmd.program == Program::Git {
                return match git {
                    Git::Lists(files) => Ok(Ran::exited(true, *files, "")),
                    Git::Fails(output) => Ok(Ran::exited(false, *output, "")),
                    Git::TimesOut => Ok(Ran::timed_out()),
                    Git::CannotStart => Err(io::Error::new(io::ErrorKind::NotFound, "no git")),
                };
            }
            if cmd.args == ["mutants", "--version"] {
                return Ok(match version {
                    Some(text) => Ran::exited(true, format!("{text}\n"), ""),
                    None => Ran::exited(
                        false,
                        "error: no such command: `mutants`\n\nhelp: ...\n",
                        "",
                    ),
                });
            }
            assert_eq!(
                std::fs::read_to_string(root.join(DIFF)).expect("the diff was saved"),
                DIFF_TEXT
            );
            assert!(
                !root.join(OUTPUT_DIR).exists(),
                "the last run's output is removed"
            );
            match run {
                Run::Writes(text, success) => {
                    Scratch::create_dir_all(root.join(OUTPUT_DIR)).expect("make mutants.out");
                    Scratch::write(root.join(OUTCOMES), text).expect("write outcomes.json");
                    Ok(Ran::exited(*success, "cargo-mutants output\n", ""))
                }
                Run::Nothing(success, output) => Ok(Ran::exited(*success, *output, "")),
                Run::TimesOut => Ok(Ran::timed_out()),
                Run::WritesThenTimesOut(text) => {
                    Scratch::create_dir_all(root.join(OUTPUT_DIR)).expect("make mutants.out");
                    Scratch::write(root.join(OUTCOMES), text).expect("write outcomes.json");
                    // The build dir a SIGKILL left behind.
                    Scratch::create_dir_all(new_build_dir.join("target"))
                        .expect("make a build dir");
                    Scratch::create_dir_all(&other_build_dir).expect("make another tree's");
                    Ok(Ran::timed_out())
                }
                Run::CannotStart => Err(io::Error::new(io::ErrorKind::NotFound, "no cargo")),
                Run::FolderForOutcomes => {
                    Scratch::create_dir_all(root.join(OUTCOMES)).expect("make a folder");
                    Ok(Ran::exited(true, String::new(), ""))
                }
            }
        };
        // A stale run's outcomes.json, which must never be read as this run's.
        Scratch::create_dir_all(root.join(OUTPUT_DIR)).expect("make the old mutants.out");
        Scratch::write(root.join(OUTCOMES), "stale").expect("write the old outcomes.json");
        run_with(&mut out, &root, &temp, diff, &mut exec)
    };
    assert!(old_build_dir.is_dir(), "an earlier run's build dir is kept");
    let other_kept = !matches!(run, Run::WritesThenTimesOut(_)) || other_build_dir.is_dir();
    assert!(other_kept, "another tree's build dir is kept");
    assert!(
        !new_build_dir.exists(),
        "a build dir the stopped run left is removed"
    );
    let printed = String::from_utf8(out.into_inner()).expect("utf-8");
    (printed, passed, ran)
}

#[test]
fn a_missed_mutant_fails_at_its_file_and_line_with_the_fix() {
    let outcomes = lab(
        &[
            baseline("Success"),
            mutant("MissedMutant", 106, 5, "replace double -> u32 with 0"),
            mutant(
                "CaughtMutant",
                110,
                5,
                "replace main -> ExitCode with Default::default()",
            ),
            mutant("Timeout", 120, 9, "replace += with -= in count"),
            mutant(
                "Unviable",
                130,
                5,
                "replace parse -> Lab with Default::default()",
            ),
        ],
        true,
    );
    let (printed, passed, ran) = check(
        "mutants-missed",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, false),
    );
    assert!(!passed);
    assert_eq!(ran, [UNTRACKED_CMD, "cargo mutants --version", CMD]);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             xtask/src/main.rs:106: missed mutant: no test fails on `replace double -> u32 with \
             0`\n{MISSED_FIX}\n\
             note: xtask/src/main.rs:120: `replace += with -= in count` made the tests run past \
             cargo-mutants' time limit, so it counts as caught\n    fix: none when the change \
             makes the code loop forever, which the tests then catch; if the tests were only \
             slow on a busy machine, run cargo xtask mutants again\n\
             note: 1 of the mutants did not compile (unviable), so they say nothing about the \
             tests; target/xtask/mutants.out/unviable.txt lists them\n    fix: none: a mutant \
             that does not compile cannot be tested, and the compiler already refuses that \
             change\n\
             mutants FAILED: 4 mutants tested: 1 missed, 1 caught, 1 timed out, 1 unviable\n"
        )
    );
}

#[test]
fn caught_and_timed_out_mutants_pass_though_cargo_mutants_exits_non_zero_on_a_timeout() {
    let outcomes = lab(
        &[
            baseline("Success"),
            mutant("CaughtMutant", 106, 5, "replace double -> u32 with 0"),
            mutant("Timeout", 120, 9, "replace += with -= in count"),
        ],
        true,
    );
    let (printed, passed, _) = check(
        "mutants-caught",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, false),
    );
    assert!(passed, "{printed}");
    assert!(
        printed.ends_with(
            "mutants ok: 2 mutants tested: 0 missed, 1 caught, 1 timed out, 0 unviable\n"
        ),
        "{printed}"
    );
}

#[test]
fn a_run_that_fails_with_clean_outcomes_and_no_timeout_still_fails() {
    let outcomes = lab(
        &[baseline("Success"), mutant("CaughtMutant", 1, 1, "x")],
        true,
    );
    let (printed, passed, _) = check(
        "mutants-odd-exit",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, false),
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\ncargo-mutants output\n\
             `{CMD}` failed, though target/xtask/mutants.out/outcomes.json shows no missed \
             mutant\n    fix: run `{CMD}` by hand to see what goes wrong\n\
             mutants FAILED: 1 mutant tested: 0 missed, 1 caught, 0 timed out, 0 unviable\n"
        )
    );
}

#[test]
fn an_empty_diff_passes_with_a_note_and_runs_nothing() {
    let (printed, passed, ran) = check(
        "mutants-empty",
        Ok("\n".into()),
        Some("cargo-mutants 27.1.0"),
        &Run::TimesOut,
    );
    assert!(passed);
    assert_eq!(ran, [UNTRACKED_CMD]);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             mutants ok: nothing changed since the merge-base with origin/main\n"
        )
    );
}

#[test]
fn a_diff_with_no_mutant_passes_and_the_stale_outcomes_are_not_read() {
    let (printed, passed, _) = check(
        "mutants-none",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Nothing(true, "INFO No mutants to filter\n"),
    );
    assert!(passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\nmutants ok: cargo-mutants found no mutant in the changed lines\n"
        )
    );
}

#[test]
fn a_run_that_fails_before_any_mutant_prints_its_output() {
    let (printed, passed, _) = check(
        "mutants-early",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Nothing(false, "ERROR Failed to parse diff: bad hunk\n"),
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\nERROR Failed to parse diff: bad hunk\n\
             `{CMD}` failed before it tested any mutant\n    fix: run `{CMD}` by hand to see what goes wrong\n"
        )
    );
}

#[test]
fn a_failed_baseline_fails_with_its_log() {
    let outcomes = lab(&[baseline("Failure")], true);
    let (printed, passed, _) = check(
        "mutants-baseline",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, false),
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             the unmutated tree failed in cargo-mutants' copy (Failure), so no mutant was \
             tested; its log is target/xtask/mutants.out/log/baseline.log\n    fix: read the \
             log: the build and tests must pass in a copy of the tree that holds .git but not \
             what .gitignore excludes, as .cargo/mutants.toml sets\n\
             mutants FAILED: 0 mutants tested: 0 missed, 0 caught, 0 timed out, 0 unviable\n"
        )
    );
}

#[test]
fn an_unfinished_run_and_an_unexpected_outcome_fail() {
    let outcomes = lab(
        &[
            baseline("Success"),
            mutant("Failure", 106, 5, "replace double -> u32 with 0"),
        ],
        false,
    );
    let (printed, passed, _) = check(
        "mutants-unfinished",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(outcomes, false),
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             xtask/src/main.rs:106: cargo-mutants gave `replace double -> u32 with 0` the \
             outcome Failure, which xtask does not expect\n    fix: read \
             target/xtask/mutants.out/log/main_106.log, then run cargo xtask mutants again\n\
             `{CMD}` stopped before its run finished: outcomes.json has no end_time\n    fix: \
             run `{CMD}` by hand to see what goes wrong\n\
             mutants FAILED: 1 mutant tested: 0 missed, 0 caught, 0 timed out, 0 unviable, 1 \
             other\n"
        )
    );
}

#[test]
fn outcomes_it_cannot_parse_fail() {
    let (printed, passed, _) = check(
        "mutants-unparsed",
        Ok(DIFF_TEXT.to_owned()),
        Some("cargo-mutants 27.1.0"),
        &Run::Writes(r#"{"outcomes": 3}"#.into(), true),
    );
    assert!(!passed);
    assert_eq!(
        printed,
        format!(
            "==> mutants: {CMD}\n\
             could not read target/xtask/mutants.out/outcomes.json: invalid type: integer `3`, \
             expected a sequence at line 1 column 14\n    fix: run cargo xtask mutants again; if \
             it fails the same way, make xtask/src/mutants/outcomes.rs read the outcomes.json \
             of the cargo-mutants version setup pins\n"
        )
    );
}

/// The real `outcomes.json` cargo-mutants 27.1.0 wrote for one mutant of the planted `double`
/// (acceptance 18), trimmed to one outcome: the field names are cargo-mutants', not ours.
#[test]
fn a_real_outcome_parses() {
    let text = r#"{"outcomes": [{"scenario": {"Mutant": {"name": "xtask/src/main.rs:106:5: replace double -> u32 with 0", "package": "xtask", "file": "xtask/src/main.rs", "function": {"function_name": "double", "return_type": "-> u32", "span": {"start": {"line": 105, "column": 1}, "end": {"line": 107, "column": 2}}}, "span": {"start": {"line": 106, "column": 5}, "end": {"line": 106, "column": 10}}, "replacement": "0", "genre": "FnValue"}}, "summary": "MissedMutant", "log_path": "log/xtask__src__main.rs_line_106_col_5.log", "diff_path": "diff/xtask__src__main.rs_line_106_col_5.diff", "phase_results": [{"phase": "Build", "duration": 3.19165075, "process_status": "Success", "argv": ["cargo", "nextest", "run", "--no-run", "--verbose", "--package=xtask@0.0.0"]}, {"phase": "Test", "duration": 13.424598833, "process_status": "Success", "argv": ["cargo", "nextest", "run", "--verbose", "--package=xtask@0.0.0", "--fail-fast"]}]}], "total_mutants": 1, "missed": 1, "caught": 0, "timeout": 0, "unviable": 0, "success": 0, "start_time": "2026-10-20T19:43:18.287389Z", "end_time": "2026-10-20T19:44:09.183114Z", "cargo_mutants_version": "27.1.0"}"#;
    let lab = outcomes::parse(text).expect("parses");
    let verdict = outcomes::judge(&lab, "cargo mutants", OUTPUT_DIR);
    let missed: Vec<String> = verdict.missed.iter().map(ToString::to_string).collect();
    assert_eq!(
        missed,
        [format!(
            "xtask/src/main.rs:106: missed mutant: no test fails on `replace double -> u32 with \
             0`\n{MISSED_FIX}"
        )]
    );
}
