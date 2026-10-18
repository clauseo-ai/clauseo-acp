//! Tests for `web.rs`: reading the script's JSON, the budgets and gating of each probe, the table,
//! and the `--json` shape of `web` and of `all`.

use super::super::profile::Incremental;
use super::super::report::{WebJson, json_object, print_failures, table_lines, web_json};
use super::super::verdict::{Limit, ProbeStatus, Stats};
use super::super::{PROBES, RustRun, rust_result};
use super::*;
use crate::budgets::Gate;
use crate::output::Printer;

fn budget(id: &str, limit: f64, unit: Unit, gate: Gate) -> Budget {
    Budget {
        id: id.into(),
        limit,
        unit,
        gate,
    }
}

/// The five rows the web half reads, as `budgets.toml` has them.
fn budgets() -> Vec<Budget> {
    vec![
        budget("tsc_noemit", 2.0, Unit::S, Gate::Local),
        budget("oxlint_type_aware", 2.0, Unit::S, Gate::Local),
        budget("vite_build_warm", 5.0, Unit::S, Gate::Local),
        budget("initial_js", 180.0, Unit::Kb, Gate::Pr),
        budget("hot_reload_component_edit", 100.0, Unit::Ms, Gate::Local),
    ]
}

const ALL_OK: &str = r#"{"node":"v24.1.0","runs":3,"probes":[
  {"probe":"tsc","samples_ms":[200,100,300]},
  {"probe":"oxlint_type_aware","samples_ms":[400,420,410]},
  {"probe":"vite_build","samples_ms":[500,700,600]},
  {"probe":"initial_js","bytes":51114,"files":["assets/index.js"]},
  {"probe":"dev_start","samples_ms":[183.4]},
  {"probe":"hmr","samples_ms":[80,100,90]}
]}"#;

fn results_of(json: &str) -> Vec<ProbeResult> {
    results(&parse_report(json).expect("valid script json"), &budgets())
}

fn find<'a>(results: &'a [ProbeResult], name: &str) -> &'a ProbeResult {
    results
        .iter()
        .find(|result| result.name == name)
        .expect("the probe is in the results")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn measured(result: &ProbeResult) -> (Stats, Option<Limit>) {
    match &result.outcome {
        Outcome::Measured { stats, budget, .. } => (*stats, *budget),
        other => panic!("{} is not measured: {other:?}", result.name),
    }
}

/// `ALL_OK` with the samples of `probe` replaced by `replacement`.
fn with(probe: &str, replacement: &str) -> String {
    let start = ALL_OK
        .find(&format!(r#"{{"probe":"{probe}""#))
        .expect("the probe is in ALL_OK");
    let end = start + ALL_OK[start..].find('}').expect("the probe closes") + 1;
    format!("{}{replacement}{}", &ALL_OK[..start], &ALL_OK[end..])
}

fn web_run(results: Vec<ProbeResult>, ci: bool) -> WebRun {
    WebRun {
        node: "v24.1.0".into(),
        runs: 3,
        ci,
        results,
    }
}

#[test]
fn numbers_convert_to_each_probes_unit_with_their_budget() {
    let results = results_of(ALL_OK);
    assert_eq!(
        results.iter().map(|result| result.name).collect::<Vec<_>>(),
        [
            "tsc",
            "oxlint_type_aware",
            "vite_build",
            "initial_js",
            "dev_start",
            "hmr"
        ]
    );
    let (tsc, tsc_budget) = measured(find(&results, "tsc"));
    assert!(close(tsc.median, 0.2) && close(tsc.min, 0.1) && close(tsc.max, 0.3));
    let tsc_budget = tsc_budget.expect("tsc has a budget");
    assert_eq!(tsc_budget.id, "tsc_noemit");
    assert!(close(tsc_budget.value, 2.0));
    let (js, js_budget) = measured(find(&results, "initial_js"));
    assert!(close(js.median, 51.114), "bytes become KB of 1000 bytes");
    assert!(close(js_budget.expect("a budget").value, 180.0));
    let (hmr, hmr_budget) = measured(find(&results, "hmr"));
    assert!(close(hmr.median, 90.0));
    assert!(close(hmr_budget.expect("a budget").value, 100.0));
    let (dev, dev_budget) = measured(find(&results, "dev_start"));
    assert!(close(dev.median, 183.4));
    assert_eq!(dev_budget, None);
    assert!(web_run(results, false).passed());
}

#[test]
fn a_failed_probe_carries_the_tools_output_and_a_command_to_reproduce_it() {
    let json = with(
        "tsc",
        r#"{"probe":"tsc","failed":{"message":"`tsc --noEmit` exited with 2","command":"pnpm -C ui/app exec tsc --noEmit","output":"src/App.tsx(3,1): error TS1005"}}"#,
    );
    let results = results_of(&json);
    let tsc = find(&results, "tsc");
    assert_eq!(tsc.status(), ProbeStatus::Failed);
    let Outcome::Failed(failure) = &tsc.outcome else {
        panic!("tsc failed");
    };
    assert_eq!(failure.message, "`tsc --noEmit` exited with 2");
    assert_eq!(
        failure.fix,
        "run `pnpm -C ui/app exec tsc --noEmit` to see the error, and fix it"
    );
    assert_eq!(
        failure.output.as_deref(),
        Some("src/App.tsx(3,1): error TS1005")
    );
    let run = web_run(results.clone(), true);
    assert!(!run.passed(), "a failure fails the run, in CI too");
    assert_eq!(
        table_lines(&results)[1],
        "tsc                failed (see below)"
    );
}

#[test]
fn a_skipped_hmr_is_reported_and_does_not_fail_the_run() {
    let json = with(
        "hmr",
        r#"{"probe":"hmr","skipped":"no Chrome or Chromium found (set CHROME_PATH)"}"#,
    );
    let results = results_of(&json);
    let hmr = find(&results, "hmr");
    assert_eq!(hmr.status(), ProbeStatus::Skipped);
    assert_eq!(
        hmr.outcome,
        Outcome::Skipped("no Chrome or Chromium found (set CHROME_PATH)".into())
    );
    assert_eq!(find(&results, "dev_start").status(), ProbeStatus::Reported);
    assert!(web_run(results, false).passed());
}

#[test]
fn a_failed_hmr_dev_start_or_initial_js_shows_failed_prints_its_lines_and_passes() {
    let cases = [
        ("hmr", 6, "hmr failed: Timeout 10000ms exceeded"),
        (
            "dev_start",
            5,
            "dev_start failed: vite exited with 1 before it was ready",
        ),
        (
            "initial_js",
            4,
            "could not size the initial JS: no module script",
        ),
    ];
    for (name, row, message) in cases {
        let failed = format!(
            r#"{{"probe":"{name}","failed":{{"message":"{message}","command":"node ui/app/scripts/buildtime.mjs --runs 3","output":"the tool's output"}}}}"#
        );
        let results = results_of(&with(name, &failed));
        let result = find(&results, name);
        assert_eq!(result.status(), ProbeStatus::Failed, "{name}");
        assert!(
            !result.fails_run(false) && !result.fails_run(true),
            "{name}"
        );
        assert!(web_run(results.clone(), false).passed(), "{name}");
        assert!(web_run(results.clone(), true).passed(), "{name} in CI");
        assert_eq!(
            table_lines(&results)[row],
            format!("{name:<19}failed (see below)")
        );
        let mut diag = Printer::new(Vec::new(), false);
        print_failures(&results, &mut diag);
        assert_eq!(
            String::from_utf8(diag.into_inner()).expect("utf-8"),
            format!(
                "the tool's output\n{message} (reported only; the run still passes)\n    fix: run `node ui/app/scripts/buildtime.mjs --runs 3` to see the error, and fix it\n"
            )
        );
        let web = WebJson::of(&Ok(web_run(results, false)));
        let json: serde_json::Value = serde_json::from_str(&web_json(&web)).expect("valid json");
        let probe = &json["web"]["probes"][row - 1];
        assert_eq!(probe["status"], "failed");
        assert_eq!(probe["reported_only"], true);
        assert_eq!(probe["reason"], message);
        assert_eq!(json["passed"], true);
    }
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

#[test]
fn a_missing_row_or_a_row_in_another_unit_fails_the_probe() {
    let mut rows = budgets();
    rows.retain(|row| row.id != "tsc_noemit");
    rows.push(budget("vite_build_warm", 5.0, Unit::Kb, Gate::Local));
    rows.retain(|row| !(row.id == "vite_build_warm" && row.unit == Unit::S));
    let results = results(&parse_report(ALL_OK).expect("valid"), &rows);
    let message = |name: &str| match &find(&results, name).outcome {
        Outcome::Failed(failure) => failure.message.clone(),
        other => panic!("{name} should fail: {other:?}"),
    };
    assert_eq!(message("tsc"), "no budget row tsc_noemit in budgets.toml");
    assert_eq!(
        message("vite_build"),
        "budget vite_build_warm is in KB, and this probe measures s"
    );
}

#[test]
fn a_limit_in_ms_converts_to_a_probe_in_seconds() {
    let mut rows = budgets();
    rows.retain(|row| row.id != "tsc_noemit");
    rows.push(budget("tsc_noemit", 1500.0, Unit::Ms, Gate::Local));
    let results = results(&parse_report(ALL_OK).expect("valid"), &rows);
    let (_, limit) = measured(find(&results, "tsc"));
    assert!(close(limit.expect("a budget").value, 1.5));
}

/// Every probe of `ALL_OK` far over its budget.
fn all_over() -> Vec<ProbeResult> {
    let tight: Vec<Budget> = budgets()
        .into_iter()
        .map(|row| Budget {
            limit: row.limit / 1000.0,
            ..row
        })
        .collect();
    results(&parse_report(ALL_OK).expect("valid"), &tight)
}

#[test]
fn over_budget_tool_probes_fail_locally_and_are_reported_in_ci() {
    let results = all_over();
    for name in ["tsc", "oxlint_type_aware", "vite_build"] {
        let result = find(&results, name);
        assert_eq!(result.status(), ProbeStatus::Over, "{name}");
        assert!(result.fails_run(false), "{name} fails locally");
        assert!(!result.fails_run(true), "{name} is reported in CI");
    }
    assert!(!web_run(results.clone(), false).passed());
    assert!(web_run(results, true).passed());
}

#[test]
fn over_budget_initial_js_hmr_and_dev_start_are_only_reported() {
    let results = all_over();
    for name in ["initial_js", "hmr"] {
        let result = find(&results, name);
        assert_eq!(result.status(), ProbeStatus::Over, "{name}");
        assert!(!result.fails_run(false), "{name} never fails locally");
        assert!(!result.fails_run(true), "{name} never fails in CI");
    }
    let initial = find(&results, "initial_js");
    assert_eq!(
        measured(initial).1.map(|budget| budget.gate),
        Some(Gate::Pr),
        "the row keeps its pr gate"
    );
    assert!(initial.reported_only, "but the probe is reported only");
    let dev = find(&results, "dev_start");
    assert_eq!(dev.status(), ProbeStatus::Reported);
    assert!(!dev.fails_run(false));
}

#[test]
fn the_table_shows_each_probe_in_its_unit_and_a_dash_without_a_budget() {
    assert_eq!(
        table_lines(&results_of(ALL_OK)),
        [
            "probe              median  min     max     budget   status",
            "tsc                0.20s   0.10s   0.30s   2.00s    ok",
            "oxlint_type_aware  0.41s   0.40s   0.42s   2.00s    ok",
            "vite_build         0.60s   0.50s   0.70s   5.00s    ok",
            "initial_js         51.1KB  51.1KB  51.1KB  180.0KB  ok",
            "dev_start          183ms   183ms   183ms   -        reported",
            "hmr                90ms    80ms    100ms   100ms    ok",
        ]
    );
    let skipped = results_of(&with(
        "hmr",
        r#"{"probe":"hmr","skipped":"no Chrome or Chromium found (set CHROME_PATH)"}"#,
    ));
    assert_eq!(
        table_lines(&skipped)[6],
        "hmr                skipped: no Chrome or Chromium found (set CHROME_PATH)"
    );
    assert_eq!(
        table_lines(&all_over())[1].split_whitespace().last(),
        Some("over")
    );
}

#[test]
fn the_web_json_carries_every_probe_with_its_unit_and_gating() {
    let web = WebJson::of(&Ok(web_run(all_over(), false)));
    let json: serde_json::Value = serde_json::from_str(&web_json(&web)).expect("valid json");
    assert_eq!(json["passed"], false, "tsc over budget fails locally");
    let half = &json["web"];
    let mut keys: Vec<&str> = half
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["ci", "node", "passed", "probes", "runs"]);
    assert_eq!(half["node"], "v24.1.0");
    assert_eq!(half["probes"].as_array().map(Vec::len), Some(6));
    let tsc = &half["probes"][0];
    let mut tsc_keys: Vec<&str> = tsc
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    tsc_keys.sort_unstable();
    assert_eq!(
        tsc_keys,
        [
            "budget",
            "budget_id",
            "max",
            "median",
            "min",
            "probe",
            "reported_only",
            "samples",
            "status",
            "unit"
        ],
        "the same shape as a Rust probe"
    );
    assert_eq!(tsc["samples"], serde_json::json!([0.2, 0.1, 0.3]));
    assert_eq!(tsc["probe"], "tsc");
    assert_eq!(tsc["status"], "over");
    assert_eq!(tsc["unit"], "s");
    assert_eq!(tsc["budget_id"], "tsc_noemit");
    assert_eq!(tsc["reported_only"], false);
    let js = &half["probes"][3];
    assert_eq!(js["unit"], "KB");
    assert_eq!(js["reported_only"], true);
    let dev = &half["probes"][4];
    assert_eq!(dev["status"], "reported");
    assert!(dev.get("budget").is_none() && dev.get("budget_id").is_none());
    assert_eq!(dev["reported_only"], true);

    let in_ci = WebJson::of(&Ok(web_run(all_over(), true)));
    assert!(in_ci.passed());
}

#[test]
fn a_web_half_that_could_not_start_prints_its_errors_and_fails() {
    let web = WebJson::of(&Err(vec!["ui/app/node_modules is missing".to_owned()]));
    assert_eq!(
        web_json(&web),
        r#"{"web":{"passed":false,"errors":["ui/app/node_modules is missing"]},"passed":false}"#
    );
}

#[test]
fn the_all_json_nests_the_web_half_under_web() {
    let rust = RustRun {
        rustc: "rustc 1.99.0".into(),
        incremental: Incremental {
            on: true,
            source: "cargo default".into(),
        },
        ci: false,
        runs: 3,
        results: vec![rust_result(
            &PROBES[0],
            Outcome::Skipped("crate not present".into()),
        )],
    };
    let web = WebJson::of(&Ok(web_run(results_of(ALL_OK), false)));
    let json: serde_json::Value =
        serde_json::from_str(&json_object(&rust, true, Some(&web))).expect("valid json");
    assert_eq!(json["rustc"], "rustc 1.99.0");
    assert_eq!(json["probes"][0]["probe"], "check_desktop");
    assert_eq!(json["web"]["probes"][5]["probe"], "hmr");
    assert_eq!(json["web"]["passed"], true);
    assert_eq!(json["passed"], true);

    let failing = WebJson::of(&Ok(web_run(all_over(), false)));
    let json: serde_json::Value =
        serde_json::from_str(&json_object(&rust, failing.passed(), Some(&failing)))
            .expect("valid json");
    assert_eq!(json["web"]["passed"], false);
    assert_eq!(json["passed"], false);
}
