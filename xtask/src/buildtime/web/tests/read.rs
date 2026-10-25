//! Tests of `web.rs` on the script's output: the last line of its stdout, its exit status, a
//! conflict over App.tsx, and the output of a real run that the parser is tied to.

use super::*;

#[test]
fn only_the_last_non_empty_line_of_stdout_is_read() {
    let noisy =
        format!("(node:123) ExperimentalWarning: a stray line\n{{\"not\": \"it\"}}\n{ALL_OK}\n\n");
    let report = parse_report(&noisy).expect("the last line is the report");
    assert_eq!(report.node, "v24.1.0");
    assert_eq!(report.probes.len(), 6);
    assert_eq!(
        parse_report("\n  \n").expect_err("nothing"),
        "ui/app/scripts/buildtime.mjs printed nothing on stdout"
    );
    let trailing = format!("{ALL_OK}\nlate noise");
    assert!(
        parse_report(&trailing).is_err(),
        "a stray last line is not the report"
    );
}

#[test]
fn json_that_xtask_cannot_read_or_a_missing_or_misshapen_probe_fails() {
    let error = parse_report("tsc: not found").expect_err("not json");
    assert!(error.starts_with("ui/app/scripts/buildtime.mjs printed JSON that xtask cannot read"));

    let missing = results_of(&with("vite_build", r#"{"probe":"other","samples_ms":[1]}"#));
    let Outcome::Failed(failure) = &find(&missing, "vite_build").outcome else {
        panic!("a missing probe fails");
    };
    assert_eq!(
        failure.message,
        "ui/app/scripts/buildtime.mjs reported no vite_build"
    );

    let misshapen = results_of(&with("tsc", r#"{"probe":"tsc","bytes":10}"#));
    assert_eq!(find(&misshapen, "tsc").status(), ProbeStatus::Failed);
    let empty = results_of(&with("tsc", r#"{"probe":"tsc","samples_ms":[]}"#));
    assert_eq!(find(&empty, "tsc").status(), ProbeStatus::Failed);
}

const CONFLICT: &str = r#"{"message":"ui/app/src/App.tsx changed during this buildtime run; its saved copy is target/xtask/buildtime/ui/app/src/App.tsx","fix":"compare the two, keep what you want, then delete target/xtask/buildtime/ui/app/src/App.tsx"}"#;

fn read(stdout: &str, success: bool) -> Read {
    read_output(
        stdout,
        success,
        "node ui/app/scripts/buildtime.mjs --runs 3",
        &budgets(),
        3,
        false,
    )
}

#[test]
fn a_conflict_during_the_run_fails_it_and_keeps_the_measurements() {
    let stdout = format!(
        r#"{},"failures":[{CONFLICT}]}}"#,
        &ALL_OK[..ALL_OK.len() - 1]
    );
    let Read::Run(run) = read(&stdout, false) else {
        panic!("a measured run");
    };
    assert!(!run.passed(), "the conflict fails the run");
    assert_eq!(run.results.len(), 6);
    assert_eq!(
        run.failures,
        [Failure::new(
            "ui/app/src/App.tsx changed during this buildtime run; its saved copy is target/xtask/buildtime/ui/app/src/App.tsx",
            "compare the two, keep what you want, then delete target/xtask/buildtime/ui/app/src/App.tsx"
        )]
    );
    let json: serde_json::Value =
        serde_json::from_str(&web_json(&WebJson::of(&Ok(run)))).expect("valid json");
    assert_eq!(json["passed"], false);
    assert_eq!(
        json["web"]["errors"][0],
        "ui/app/src/App.tsx changed during this buildtime run; its saved copy is target/xtask/buildtime/ui/app/src/App.tsx"
    );
}

#[test]
fn a_conflict_left_by_an_interrupted_run_stops_the_half() {
    let stdout = format!(r#"{{"node":"v24.1.0","runs":3,"probes":[],"failures":[{CONFLICT}]}}"#);
    let Read::Stopped(failures) = read(&stdout, false) else {
        panic!("a stopped half");
    };
    assert_eq!(failures.len(), 1);
    assert!(failures[0].fix.starts_with("compare the two"));
}

#[test]
fn a_non_zero_exit_fails_the_run_even_with_a_report() {
    let Read::Run(run) = read(ALL_OK, false) else {
        panic!("a measured run");
    };
    assert!(!run.passed());
    assert_eq!(
        run.failures[0].message,
        "`node ui/app/scripts/buildtime.mjs --runs 3` exited non-zero after it reported"
    );
    let Read::Run(clean) = read(ALL_OK, true) else {
        panic!("a measured run");
    };
    assert!(clean.passed() && clean.failures.is_empty());
    assert!(matches!(read("Error: boom", false), Read::Unreadable(_)));
}

/// The stdout of a real run, so the parser is tied to what the script really prints. Refresh it
/// after changing the script's output: from the repo root, with Chrome installed and with no other
/// buildtime run going (this runs the script without xtask's lock, which covers App.tsx),
/// `node ui/app/scripts/buildtime.mjs --runs 1 > xtask/src/buildtime/web/script-output.json`.
const REAL_OUTPUT: &str = include_str!("../script-output.json");

#[test]
fn the_output_of_a_real_script_run_reads_into_all_six_probes() {
    let Read::Run(run) = read_output(REAL_OUTPUT, true, "node", &budgets(), 1, false) else {
        panic!("the real output is a measured run");
    };
    let names: Vec<&str> = run.results.iter().map(|result| result.name).collect();
    assert_eq!(
        names,
        WEB_PROBES
            .iter()
            .map(|probe| probe.name)
            .collect::<Vec<_>>()
    );
    for result in &run.results {
        assert!(
            matches!(result.outcome, Outcome::Measured { .. }),
            "{} was measured in the real run: {:?}",
            result.name,
            result.outcome
        );
    }
    assert_eq!(run.failures, []);
}
