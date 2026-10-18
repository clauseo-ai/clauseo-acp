//! Tests for `budgets.rs`: the rules, one per failure, and the real file.

use super::*;

/// A complete, valid row: its header is line 1.
const ROW: &str = "[[budget]]
id = \"cold_start\"
metric = \"cold start to interactive\"
limit = 1500.0
unit = \"ms\"
gate = \"release\"
measured_by = \"marks across Rust and JS\"
status = \"unmeasured\"
source = \"synthesis §4\"
";

fn only_violations(text: &str) -> Vec<Violation> {
    let (budgets, violations) = parse(text);
    assert!(budgets.is_empty(), "expected no rows, got {budgets:?}");
    violations
}

fn one_violation(text: &str) -> Violation {
    let violations = only_violations(text);
    assert_eq!(violations.len(), 1, "{violations:?}");
    violations[0].clone()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_valid_row_parses_with_every_field() {
    let text = ROW.replace(
        "status = \"unmeasured\"\n",
        "status = \"measured\"\nmeasured = 1200\nmeasured_on = \"macos-arm64-laptop\"\n",
    );
    let (budgets, violations) = parse(&text);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(budgets.len(), 1);
    let row = &budgets[0];
    assert_eq!(row.id, "cold_start");
    assert_eq!(row.metric, "cold start to interactive");
    assert!(close(row.limit, 1500.0));
    assert_eq!(row.unit, Unit::Ms);
    assert_eq!(row.gate, Gate::Release);
    assert_eq!(row.measured_by, "marks across Rust and JS");
    assert_eq!(row.status, Status::Measured);
    assert!(row.measured.is_some_and(|m| close(m, 1200.0)));
    assert_eq!(row.measured_on.as_deref(), Some("macos-arm64-laptop"));
    assert_eq!(row.source, "synthesis §4");
    assert_eq!(row.line, 1);
}

#[test]
fn an_unmeasured_row_has_no_measured_fields() {
    let (budgets, violations) = parse(ROW);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(budgets[0].status, Status::Unmeasured);
    assert!(budgets[0].measured.is_none());
    assert!(budgets[0].measured_on.is_none());
}

#[test]
fn a_comment_after_the_header_and_after_values_is_allowed() {
    let text = "[[budget]] # the first\nid = \"a_b\" # name\nmetric = \"m\"\nlimit = 0 # zero\nunit = \"count\"\ngate = \"alert\"\nmeasured_by = \"x\"\nstatus = \"unmeasured\"\nsource = \"s\"\n";
    let (budgets, violations) = parse(text);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(budgets[0].unit, Unit::Count);
    assert_eq!(budgets[0].gate, Gate::Alert);
}

#[test]
fn removing_gate_reports_exactly_that_at_the_header() {
    let text = ROW.replace("gate = \"release\"\n", "");
    let violation = one_violation(&text);
    assert_eq!(
        violation,
        Violation {
            path: "budgets.toml".into(),
            line: 1,
            message: "budget cold_start has no gate".into(),
            fix: "add gate = \"pr\", \"release\", \"alert\" or \"local\"".into(),
        }
    );
}

#[test]
fn a_row_without_an_id_is_reported_at_its_header() {
    let text = ROW.replace("id = \"cold_start\"\n", "");
    let violation = one_violation(&text);
    assert_eq!(violation.line, 1);
    assert_eq!(violation.message, "budget has no id");
    assert_eq!(
        violation.fix,
        "add id = \"snake_case_name\", unique in budgets.toml"
    );
}

#[test]
fn a_duplicate_id_is_reported_at_the_second_id_line() {
    let text = format!("{ROW}{ROW}");
    let (budgets, violations) = parse(&text);
    assert_eq!(budgets.len(), 1, "the first row is still a budget");
    assert_eq!(violations.len(), 1, "{violations:?}");
    let violation = &violations[0];
    assert_eq!(violation.line, 11);
    assert_eq!(
        violation.message,
        "budget cold_start is defined twice (first at line 1)"
    );
}

#[test]
fn an_unknown_key_lists_the_ten_keys_in_its_fix() {
    let text = ROW.replace(
        "source = \"synthesis §4\"\n",
        "source = \"synthesis §4\"\ncolour = \"red\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 10);
    assert_eq!(violation.message, "budget cold_start: unknown key colour");
    assert!(violation.fix.starts_with("use one of the ten keys:"));
    assert!(violation.fix.contains("measured_on, source"));
}

#[test]
fn a_unit_outside_the_set_is_reported_at_its_line() {
    let text = ROW.replace("unit = \"ms\"", "unit = \"seconds\"");
    let violation = one_violation(&text);
    assert_eq!(violation.line, 5);
    assert_eq!(
        violation.message,
        "budget cold_start: unit \"seconds\" is not one of s, ms, MB, KB, count"
    );
    assert_eq!(
        violation.fix,
        "set unit to one of \"s\", \"ms\", \"MB\", \"KB\", \"count\""
    );
}

#[test]
fn a_gate_and_a_status_outside_their_sets_are_reported() {
    let gate = one_violation(&ROW.replace("gate = \"release\"", "gate = \"nightly\""));
    assert_eq!(
        gate.message,
        "budget cold_start: gate \"nightly\" is not one of pr, release, alert, local"
    );
    let status = one_violation(&ROW.replace("status = \"unmeasured\"", "status = \"done\""));
    assert_eq!(
        status.message,
        "budget cold_start: status \"done\" is not one of unmeasured, measured"
    );
}

#[test]
fn a_string_where_a_number_goes_is_reported() {
    let violation = one_violation(&ROW.replace("limit = 1500.0", "limit = \"1500\""));
    assert_eq!(violation.line, 4);
    assert_eq!(
        violation.message,
        "budget cold_start: limit must be a number"
    );
}

#[test]
fn a_number_where_a_string_goes_is_reported() {
    let violation =
        one_violation(&ROW.replace("metric = \"cold start to interactive\"", "metric = 4"));
    assert_eq!(
        violation.message,
        "budget cold_start: metric must be a string"
    );
}

#[test]
fn a_negative_or_huge_limit_is_reported() {
    let negative = one_violation(&ROW.replace("limit = 1500.0", "limit = -1.0"));
    assert_eq!(
        negative.message,
        "budget cold_start: limit must be zero or more"
    );
    let huge =
        one_violation(&ROW.replace("limit = 1500.0", &format!("limit = 1{}", "0".repeat(400))));
    assert_eq!(
        huge.message,
        "budget cold_start: limit must be zero or more"
    );
}

#[test]
fn a_key_given_twice_is_reported_at_the_second_line() {
    let text = ROW.replace(
        "source = \"synthesis §4\"\n",
        "source = \"a\"\nsource = \"b\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 10);
    assert_eq!(violation.message, "budget cold_start: source appears twice");
}

#[test]
fn a_snake_case_id_is_required() {
    let violation = one_violation(&ROW.replace("id = \"cold_start\"", "id = \"Cold-Start\""));
    assert_eq!(violation.line, 2);
    assert_eq!(
        violation.message,
        "budget Cold-Start: id must be snake_case"
    );
}

#[test]
fn measured_on_must_be_lowercase_words_joined_by_dashes() {
    let text = ROW.replace(
        "status = \"unmeasured\"\n",
        "status = \"measured\"\nmeasured = 2\nmeasured_on = \"Mac Book\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 10);
    assert_eq!(
        violation.message,
        "budget cold_start: measured_on must be lowercase words joined by -"
    );
}

#[test]
fn measured_without_measured_on_is_reported_at_the_header() {
    let text = ROW.replace(
        "status = \"unmeasured\"\n",
        "status = \"unmeasured\"\nmeasured = 2\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 1);
    assert_eq!(
        violation.message,
        "budget cold_start has measured but no measured_on"
    );
}

#[test]
fn measured_on_without_measured_is_reported_at_the_header() {
    let text = ROW.replace(
        "status = \"unmeasured\"\n",
        "status = \"unmeasured\"\nmeasured_on = \"macos-arm64\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 1);
    assert_eq!(
        violation.message,
        "budget cold_start has measured_on but no measured"
    );
}

#[test]
fn a_measured_status_without_a_value_is_reported_at_the_header() {
    let text = ROW.replace("status = \"unmeasured\"\n", "status = \"measured\"\n");
    let violation = one_violation(&text);
    assert_eq!(violation.line, 1);
    assert_eq!(
        violation.message,
        "budget cold_start is measured but has no measured value"
    );
}

#[test]
fn a_line_it_cannot_read_is_reported_and_its_row_dropped() {
    let text = format!("{ROW}this is not a key\n");
    let (budgets, violations) = parse(&text);
    assert_eq!(budgets.len(), 0);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].line, 10);
    assert_eq!(violations[0].message, "cannot read this line");
    assert_eq!(violations[0].fix, FORMAT_FIX);
}

#[test]
fn unquoted_text_and_bad_escapes_cannot_be_read() {
    let cases = [
        (ROW.replace("unit = \"ms\"", "unit = ms"), 5),
        (ROW.replace("unit = \"ms\"", "unit = \"a\\n\""), 5),
        (ROW.replace("limit = 1500.0", "limit = 1e3"), 4),
    ];
    for (text, line) in cases {
        let violations = only_violations(&text);
        assert!(
            violations
                .iter()
                .any(|v| v.line == line && v.message == "cannot read this line"),
            "{violations:?}"
        );
    }
}

#[test]
fn a_key_before_the_first_header_is_outside_a_table() {
    let text = format!("id = \"x\"\n{ROW}");
    let (budgets, violations) = parse(&text);
    assert_eq!(budgets.len(), 1, "the table after it still parses");
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].line, 1);
    assert_eq!(
        violations[0].message,
        "key id is outside a [[budget]] table"
    );
    assert_eq!(violations[0].fix, "move it under a [[budget]] header");
}

#[test]
fn a_file_without_tables_is_reported_on_line_1() {
    let violations = only_violations("# just a comment\n");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].line, 1);
    assert_eq!(violations[0].message, "no [[budget]] tables");
}

#[test]
fn a_missing_file_is_one_violation_on_line_1() {
    let violations = load(Path::new("/nonexistent/xtask-budgets-test"))
        .err()
        .unwrap_or_default();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].path, "budgets.toml");
    assert_eq!(violations[0].line, 1);
    assert!(
        violations[0]
            .message
            .starts_with("cannot read budgets.toml: ")
    );
    assert_eq!(violations[0].fix, "restore budgets.toml at the repo root");
}

// The repo's own budgets.toml is checked by the `budgets` step of `cargo xtask check`, not by a
// test here, so a bad row is reported once, with its file and line.
