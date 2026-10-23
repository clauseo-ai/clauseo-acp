//! Tests for `buildtime.rs`: the statistics, the verdict, the plan, the budget lookup and the
//! output.

use super::report::{error_json, json_object, probe_json, table_lines, warning_lines};
use super::verdict::*;
use super::*;
use crate::budgets::{Budget, Gate, Unit};
use crate::process::Ran;
use crate::scratch::Scratch;

fn member(name: &str, dir: &str) -> Member {
    Member {
        name: name.into(),
        dir: dir.into(),
        path_deps: Vec::new(),
        targets: Vec::new(),
        dependencies: crate::workspace::PackageDependencies::default(),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn measured(median: f64, budget_s: f64) -> ProbeResult {
    measured_gated(Gate::Local, median, budget_s)
}

fn measured_gated(gate: Gate, median: f64, budget_s: f64) -> ProbeResult {
    probe_named(
        "check_desktop",
        Outcome::Measured {
            samples: vec![median],
            stats: Stats {
                median,
                min: median,
                max: median,
            },
            budget: Some(Limit {
                id: "cargo_check_app_edit",
                value: budget_s,
                gate,
            }),
            warning: None,
        },
    )
}

fn probe_named(name: &'static str, outcome: Outcome) -> ProbeResult {
    rust_result(
        &Probe {
            name,
            package: "clauseo-desktop",
            budget_id: "cargo_check_app_edit",
            subcommand: "check",
        },
        outcome,
    )
}

fn over_budget(name: &'static str, gate: Gate) -> ProbeResult {
    let mut result = measured_gated(gate, 2.0, 1.0);
    result.name = name;
    result
}

fn skipped() -> ProbeResult {
    probe_named(
        "check_desktop",
        Outcome::Skipped("crate not present".into()),
    )
}

fn failed() -> ProbeResult {
    probe_named(
        "check_desktop",
        Outcome::Failed(Failure::new("cargo check failed", "run `cargo check`")),
    )
}

#[test]
fn stats_of_an_odd_count_take_the_middle_value() {
    let stats = stats(&[3.0, 1.0, 2.0]).expect("three samples");
    assert!(close(stats.median, 2.0));
    assert!(close(stats.min, 1.0));
    assert!(close(stats.max, 3.0));
}

#[test]
fn stats_of_an_even_count_average_the_middle_two() {
    let stats = stats(&[4.0, 1.0, 3.0, 2.0]).expect("four samples");
    assert!(close(stats.median, 2.5));
    assert!(close(stats.min, 1.0));
    assert!(close(stats.max, 4.0));
}

#[test]
fn stats_of_no_samples_is_none() {
    assert_eq!(stats(&[]), None);
}

#[test]
fn a_probe_within_its_budget_is_ok_and_one_over_it_is_over() {
    assert_eq!(measured(0.5, 1.0).status(), ProbeStatus::Ok);
    assert_eq!(measured(1.0, 1.0).status(), ProbeStatus::Ok);
    assert_eq!(measured(1.5, 1.0).status(), ProbeStatus::Over);
}

#[test]
fn an_over_budget_local_probe_fails_locally_and_passes_in_ci() {
    let results = [
        measured_gated(Gate::Local, 0.5, 1.0),
        over_budget("build_desktop", Gate::Local),
    ];
    assert!(!passes(&results, false));
    assert!(passes(&results, true));
}

#[test]
fn every_gate_decides_an_over_budget_probe_with_and_without_ci() {
    assert!(over_budget_fails(Gate::Local, false));
    assert!(!over_budget_fails(Gate::Local, true));
    assert!(!over_budget_fails(Gate::Alert, false));
    assert!(!over_budget_fails(Gate::Alert, true));
    assert!(over_budget_fails(Gate::Pr, false));
    assert!(over_budget_fails(Gate::Pr, true));
    assert!(over_budget_fails(Gate::Release, false));
    assert!(over_budget_fails(Gate::Release, true));
}

#[test]
fn an_over_budget_probe_fails_the_run_only_when_its_gate_says_so() {
    for ci in [false, true] {
        assert!(
            !passes(&[over_budget("x", Gate::Pr)], ci),
            "pr fails with ci={ci}"
        );
        assert!(
            !passes(&[over_budget("x", Gate::Release)], ci),
            "release fails with ci={ci}"
        );
        assert!(
            passes(&[over_budget("x", Gate::Alert)], ci),
            "alert passes with ci={ci}"
        );
        assert_eq!(
            passes(&[over_budget("x", Gate::Local)], ci),
            ci,
            "local passes only in ci={ci}"
        );
    }
}

#[test]
fn a_failed_probe_fails_even_in_ci() {
    let failed = failed();
    assert!(!passes(std::slice::from_ref(&failed), true));
    assert!(!passes(std::slice::from_ref(&failed), false));
    let with_over = [over_budget("build_desktop", Gate::Local), failed];
    assert!(!passes(&with_over, true));
}

#[test]
fn skipped_and_ok_probes_pass() {
    assert!(passes(&[skipped(), measured(0.5, 1.0)], false));
    assert!(passes(&[], false));
}

#[test]
fn a_probe_that_was_not_incremental_warns_under_the_table() {
    let mut warned = measured(0.5, 1.0);
    if let Outcome::Measured { warning, .. } = &mut warned.outcome {
        *warning = Some("warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget".into());
    }
    assert_eq!(
        warning_lines(&[measured(0.5, 1.0), warned]),
        vec!["warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget".to_owned()]
    );
    assert_eq!(warning_lines(&[measured(0.5, 1.0)]), Vec::<String>::new());
}

#[test]
fn probes_whose_package_is_missing_are_planned_as_skipped() {
    let members = vec![member("clauseo-protocol", "crates/protocol")];
    let planned = plan(PROBES, &members);
    assert_eq!(planned.len(), 4);
    let skipped = planned
        .iter()
        .filter(|p| matches!(p, Planned::Skip(_)))
        .count();
    assert_eq!(skipped, 2);
    assert!(matches!(
        &planned[0],
        Planned::Skip(probe) if probe.name == "check_desktop"
    ));
    assert!(matches!(
        &planned[2],
        Planned::Run { probe, dir } if probe.name == "check_protocol" && dir == "crates/protocol"
    ));
}

#[test]
fn probes_find_their_marker_in_the_member_directory_but_run_at_the_root() {
    let members = vec![member("clauseo-desktop", "crates/desktop")];
    let planned = plan(&PROBES[..2], &members);
    assert!(matches!(
        &planned[1],
        Planned::Run { probe, dir } if probe.subcommand == "build" && dir == "crates/desktop"
    ));
}

#[test]
fn a_limit_converts_to_the_unit_the_probe_measures() {
    let budgets = vec![budget("a", Unit::S, 5.0), budget("b", Unit::Ms, 250.0)];
    let seconds = budget_limit(&budgets, "a", Unit::S).expect("a row in seconds");
    assert!(close(seconds.value, 5.0));
    assert_eq!(seconds.id, "a");
    let millis = budget_limit(&budgets, "b", Unit::S).expect("a row in ms");
    assert!(close(millis.value, 0.25));
    let back = budget_limit(&budgets, "a", Unit::Ms).expect("seconds to ms");
    assert!(close(back.value, 5000.0));
    let kb = budget_limit(&[budget("k", Unit::Kb, 180.0)], "k", Unit::Kb).expect("KB to KB");
    assert!(close(kb.value, 180.0));
}

#[test]
fn a_limit_carries_its_row_gate() {
    let mut row = budget("a", Unit::S, 5.0);
    row.gate = Gate::Release;
    let limit = budget_limit(&[row], "a", Unit::S).expect("a row in seconds");
    assert_eq!(limit.gate, Gate::Release);
}

#[test]
fn a_missing_row_or_another_unit_fails_the_probe_clearly() {
    let budgets = vec![budget("mb", Unit::Mb, 60.0)];
    let missing = budget_limit(&budgets, "nope", Unit::S).expect_err("no such row");
    assert_eq!(missing.message, "no budget row nope in budgets.toml");
    assert_eq!(
        missing.fix,
        "add a [[budget]] row with id = \"nope\" to budgets.toml"
    );
    let unit = budget_limit(&budgets, "mb", Unit::S).expect_err("a unit in MB");
    assert_eq!(
        unit.message,
        "budget mb is in MB, and this probe measures s"
    );
    assert_eq!(unit.fix, "set unit = \"s\" on budget mb");
}

fn budget(id: &str, unit: Unit, limit: f64) -> Budget {
    Budget {
        id: id.into(),
        limit,
        unit,
        gate: budgets::Gate::Pr,
    }
}

#[test]
fn a_skipped_probe_prints_its_reason_in_the_table() {
    let lines = table_lines(&[skipped()]);
    assert!(lines[0].starts_with("probe"));
    assert_eq!(lines[1], "check_desktop  skipped: crate not present");
}

#[test]
fn a_measured_probe_prints_seconds_and_its_status() {
    let lines = table_lines(&[measured(0.52, 1.0)]);
    assert!(lines[0].starts_with("probe"));
    assert!(lines[0].ends_with("status"));
    assert_eq!(lines[1], "check_desktop  0.52s   0.52s  0.52s  1.00s   ok");
}

#[test]
fn a_failed_probe_points_at_the_failure_below_the_table() {
    let lines = table_lines(&[failed()]);
    assert_eq!(lines[1], "check_desktop  failed (see below)");
}

#[test]
fn the_json_of_a_skipped_probe_has_only_its_state_and_reason() {
    let json = serde_json::to_string(&probe_json(&skipped())).unwrap_or_default();
    assert_eq!(
        json,
        r#"{"probe":"check_desktop","status":"skipped","unit":"s","reported_only":false,"reason":"crate not present"}"#
    );
}

#[test]
fn the_json_of_a_measured_probe_has_its_unit_stats_samples_and_budget() {
    let json = serde_json::to_value(probe_json(&measured(0.5, 1.0))).unwrap_or_default();
    assert_eq!(
        json.to_string(),
        r#"{"budget":1.0,"budget_id":"cargo_check_app_edit","max":0.5,"median":0.5,"min":0.5,"probe":"check_desktop","reported_only":false,"samples":[0.5],"status":"ok","unit":"s"}"#
    );
}

fn rust_run(results: Vec<ProbeResult>, ci: bool) -> RustRun {
    RustRun {
        rustc: "rustc 1.99.0".into(),
        incremental: Incremental {
            on: true,
            source: "cargo default".into(),
        },
        ci,
        runs: 3,
        results,
    }
}

#[test]
fn the_json_object_has_the_run_count_and_the_verdict() {
    let run = rust_run(vec![measured(0.5, 1.0), skipped()], false);
    let json: serde_json::Value =
        serde_json::from_str(&json_object(&run, true, None)).expect("valid json");
    assert_eq!(json["runs"], 3);
    assert_eq!(json["passed"], true);
    assert_eq!(json["probes"][1]["status"], "skipped");
    assert!(json.get("web").is_none());

    let failing = rust_run(vec![over_budget("build_desktop", Gate::Pr)], false);
    let json: serde_json::Value =
        serde_json::from_str(&json_object(&failing, false, None)).expect("valid json");
    assert_eq!(json["passed"], false);
}

#[test]
fn the_error_json_is_one_line_per_error_and_never_empty() {
    assert_eq!(
        error_json(&["budgets.toml:39: budget x has no gate".to_owned()]),
        r#"{"passed":false,"errors":["budgets.toml:39: budget x has no gate"]}"#
    );
    assert_eq!(error_json(&[]), r#"{"passed":false,"errors":[]}"#);
}

#[test]
fn a_stopped_run_prints_its_reason_on_one_line() {
    let errors = [one_line("refusing to measure:\n  CI is set")];
    assert_eq!(errors[0], "refusing to measure: CI is set");
}

#[test]
fn the_json_of_a_probe_carries_its_warning_only_when_there_is_one() {
    let mut warned = measured(0.5, 1.0);
    if let Outcome::Measured { warning, .. } = &mut warned.outcome {
        *warning = Some("warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget".into());
    }
    let json = serde_json::to_value(probe_json(&warned)).unwrap_or_default();
    assert_eq!(
        json["warning"],
        "warning: clauseo-desktop builds without incremental; its timing is not comparable with the budget"
    );
    let plain = serde_json::to_value(probe_json(&measured(0.5, 1.0))).unwrap_or_default();
    assert!(plain.get("warning").is_none());
}

#[test]
fn the_profile_line_is_an_expectation_that_the_warm_up_decides() {
    let off = Incremental {
        on: false,
        source: profile::CI_SOURCE.to_owned(),
    };
    assert_eq!(
        profile_line(&off, PROBE_DECIDER),
        format!(
            "profile: dev, incremental expected off ({}); each probe's warm-up rustc line decides",
            profile::CI_SOURCE
        )
    );
}

#[test]
fn rustc_version_is_the_line_rustc_prints_trimmed() {
    let mut runner = |cmd: &Cmd, _: &Path| -> io::Result<Ran> {
        assert_eq!(cmd.to_string(), "rustc -V");
        Ok(Ran::exited(
            true,
            "rustc 1.90.0 (1159e78c4 2025-09-14)\n",
            String::new(),
        ))
    };
    assert_eq!(
        rustc_version(Path::new("."), &mut runner),
        Ok("rustc 1.90.0 (1159e78c4 2025-09-14)".to_owned())
    );
}

#[test]
fn a_rustc_version_that_exits_non_zero_is_named_with_its_stderr() {
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<Ran> {
        Ok(Ran::exited(
            false,
            String::new(),
            "error: no toolchain installed\n",
        ))
    };
    assert_eq!(
        rustc_version(Path::new("."), &mut runner),
        Err(Failure::new(
            "`rustc -V` failed: error: no toolchain installed",
            "install Rust with rustup, then run `rustup show`",
        ))
    );
}

#[test]
fn a_rustc_version_past_its_limit_says_so_and_how_to_see_why() {
    let mut runner = |_: &Cmd, _: &Path| -> io::Result<Ran> { Ok(Ran::timed_out()) };
    assert_eq!(
        rustc_version(Path::new("."), &mut runner),
        Err(Failure::new(
            "`rustc -V` did not finish within 10 min, so it was stopped",
            "run `rustc -V` by hand to see why it does not finish within 10 min",
        ))
    );
}

#[test]
fn a_rustc_that_cannot_start_is_named_with_the_rustup_fix() {
    let mut runner =
        |_: &Cmd, _: &Path| -> io::Result<Ran> { Err(io::Error::other("no such file")) };
    assert_eq!(
        rustc_version(Path::new("."), &mut runner),
        Err(Failure::new(
            "could not run `rustc -V`: no such file",
            "install Rust with rustup",
        ))
    );
}

#[test]
fn each_part_runs_its_halves_and_worktree_runs_neither() {
    assert_eq!(halves(Part::Rust), Some((true, false)));
    assert_eq!(halves(Part::Web), Some((false, true)));
    assert_eq!(halves(Part::All), Some((true, true)));
    assert_eq!(halves(Part::Worktree), None);
}

#[test]
fn a_run_whose_checkout_has_no_budgets_fails_before_it_measures() {
    let root = Scratch::new("buildtime-run");
    assert_eq!(run(&root, Part::Web, 1, false), ExitCode::FAILURE);
}
