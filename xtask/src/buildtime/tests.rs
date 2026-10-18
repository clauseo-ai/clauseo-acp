//! Tests for `buildtime.rs`: the statistics, the verdict, the plan, the budget lookup and the output.

use super::report::{probe_json, table_lines, web_json};
use super::*;

fn member(name: &str, dir: &str) -> Member {
    Member {
        name: name.into(),
        dir: dir.into(),
        path_deps: Vec::new(),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn measured(median: f64, budget_s: f64) -> ProbeResult {
    ProbeResult {
        name: "check_desktop",
        outcome: Outcome::Measured {
            stats: Stats {
                median,
                min: median,
                max: median,
            },
            budget_id: "cargo_check_app_edit",
            budget_s,
        },
    }
}

fn skipped() -> ProbeResult {
    ProbeResult {
        name: "check_desktop",
        outcome: Outcome::Skipped("crate not present"),
    }
}

fn failed() -> ProbeResult {
    ProbeResult {
        name: "check_desktop",
        outcome: Outcome::Failed(Failure::new("cargo check failed", "run `cargo check`")),
    }
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
    assert_eq!(measured(0.5, 1.0).outcome.status(), ProbeStatus::Ok);
    assert_eq!(measured(1.0, 1.0).outcome.status(), ProbeStatus::Ok);
    assert_eq!(measured(1.5, 1.0).outcome.status(), ProbeStatus::Over);
}

#[test]
fn an_over_budget_run_fails_locally_and_passes_in_ci() {
    let statuses = [ProbeStatus::Ok, ProbeStatus::Over];
    assert!(!passes(&statuses, false));
    assert!(passes(&statuses, true));
}

#[test]
fn a_failed_probe_fails_even_in_ci() {
    assert!(!passes(&[ProbeStatus::Failed], true));
    assert!(!passes(&[ProbeStatus::Over, ProbeStatus::Failed], true));
    assert!(!passes(&[ProbeStatus::Failed], false));
}

#[test]
fn skipped_and_ok_probes_pass() {
    assert!(passes(&[ProbeStatus::Skipped, ProbeStatus::Ok], false));
    assert!(passes(&[], false));
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
fn probes_run_in_the_member_directory_when_it_is_present() {
    let members = vec![member("clauseo-desktop", "crates/desktop")];
    let planned = plan(&PROBES[..2], &members);
    assert!(matches!(
        &planned[1],
        Planned::Run { probe, dir } if probe.subcommand == "build" && dir == "crates/desktop"
    ));
}

#[test]
fn a_limit_in_seconds_or_milliseconds_converts_to_seconds() {
    let budgets = vec![budget("a", Unit::S, 5.0), budget("b", Unit::Ms, 250.0)];
    assert!(close(budget_seconds(&budgets, "a").unwrap_or(0.0), 5.0));
    assert!(close(budget_seconds(&budgets, "b").unwrap_or(0.0), 0.25));
}

#[test]
fn a_missing_row_or_another_unit_fails_the_probe_clearly() {
    let budgets = vec![budget("mb", Unit::Mb, 60.0)];
    let missing = budget_seconds(&budgets, "nope")
        .err()
        .unwrap_or_else(|| Failure::new("", ""));
    assert_eq!(missing.message, "no budget row nope in budgets.toml");
    let unit = budget_seconds(&budgets, "mb")
        .err()
        .unwrap_or_else(|| Failure::new("", ""));
    assert_eq!(
        unit.message,
        "budget mb is in MB, and buildtime compares s or ms"
    );
}

fn budget(id: &str, unit: Unit, limit: f64) -> Budget {
    Budget {
        id: id.into(),
        metric: "m".into(),
        limit,
        unit,
        gate: budgets::Gate::Pr,
        measured_by: "x".into(),
        status: budgets::Status::Unmeasured,
        measured: None,
        measured_on: None,
        source: "s".into(),
        line: 1,
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
        r#"{"probe":"check_desktop","status":"skipped","reason":"crate not present"}"#
    );
}

#[test]
fn the_json_of_a_measured_probe_has_its_stats_and_budget() {
    let json = serde_json::to_value(probe_json(&measured(0.5, 1.0))).unwrap_or_default();
    assert_eq!(json["status"], "ok");
    assert_eq!(json["budget_id"], "cargo_check_app_edit");
    assert_eq!(json["budget_s"], 1.0);
    assert_eq!(json["median_s"], 0.5);
    assert!(json.get("reason").is_none());
}

#[test]
fn the_web_only_json_is_one_field() {
    assert_eq!(web_json(), r#"{"web":"not implemented until R4"}"#);
}
