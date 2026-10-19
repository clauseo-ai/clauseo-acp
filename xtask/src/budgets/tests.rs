//! Tests for `budgets.rs`: what `toml` accepts, each rule the file adds, and the real file.

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
    let Read {
        budgets,
        violations,
        ..
    } = parse_counted(text);
    assert!(budgets.is_empty(), "expected no rows, got {budgets:?}");
    violations
}

fn one_violation(text: &str) -> Violation {
    let violations = only_violations(text);
    assert_eq!(violations.len(), 1, "{violations:?}");
    violations[0].clone()
}

fn valid_row(text: &str) -> Budget {
    let Read {
        budgets,
        violations,
        ..
    } = parse_counted(text);
    assert!(violations.is_empty(), "{violations:?}");
    assert_eq!(budgets.len(), 1, "{budgets:?}");
    budgets[0].clone()
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
    let row = valid_row(&text);
    assert_eq!(row.id, "cold_start");
    assert!(close(row.limit, 1500.0));
    assert_eq!(row.unit, Unit::Ms);
    assert_eq!(row.gate, Gate::Release);
}

#[test]
fn an_unmeasured_row_parses_and_keeps_only_what_buildtime_reads() {
    let row = valid_row(ROW);
    assert_eq!(
        row,
        Budget {
            id: "cold_start".into(),
            limit: 1500.0,
            unit: Unit::Ms,
            gate: Gate::Release,
        }
    );
}

#[test]
fn an_integer_limit_reads_as_a_number() {
    let row = valid_row(&ROW.replace("limit = 1500.0", "limit = 15"));
    assert!(close(row.limit, 15.0), "{}", row.limit);
}

#[test]
fn valid_toml_the_old_reader_rejected_now_parses() {
    let cases = [
        ROW.replace("limit = 1500.0", "limit = 15_000"),
        ROW.replace("limit = 1500.0", "limit = 1e3"),
        ROW.replace("limit = 1500.0", "limit = +1.0"),
        ROW.replace("gate = \"release\"", "\"gate\" = \"release\""),
        ROW.replace("metric = \"cold start to interactive\"", "metric = 'x'"),
        ROW.replace("[[budget]]", "[[ budget ]]"),
    ];
    for text in cases {
        let row = valid_row(&text);
        assert_eq!(row.id, "cold_start");
    }
    assert!(close(
        valid_row(&ROW.replace("limit = 1500.0", "limit = 15_000")).limit,
        15_000.0
    ));
    assert!(close(
        valid_row(&ROW.replace("limit = 1500.0", "limit = 1e3")).limit,
        1000.0
    ));
}

#[test]
fn a_leading_zero_is_invalid_toml() {
    let violation = one_violation(&ROW.replace("limit = 1500.0", "limit = 01"));
    assert_eq!(violation.line, 4);
    assert!(
        violation.message.starts_with("budgets.toml is not valid: "),
        "{violation:?}"
    );
    assert!(violation.fix.starts_with("fix the line above;"));
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
    let Read {
        budgets,
        violations,
        ..
    } = parse_counted(&text);
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
fn an_unknown_key_is_one_violation_at_its_line() {
    let text = ROW.replace(
        "source = \"synthesis §4\"\n",
        "source = \"synthesis §4\"\ncolour = \"red\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 10);
    assert!(
        violation.message.starts_with("budgets.toml is not valid: "),
        "{violation:?}"
    );
    assert!(violation.message.contains("colour"), "{violation:?}");
    assert!(violation.message.contains("measured_on"), "{violation:?}");
}

#[test]
fn a_misspelt_key_names_the_key_and_its_line() {
    let violation = one_violation(&ROW.replace("gate = \"release\"", "gatee = \"pr\""));
    assert_eq!(violation.line, 6);
    assert!(violation.message.contains("gatee"), "{violation:?}");
}

#[test]
fn a_unit_outside_the_set_is_one_violation_at_its_line() {
    let violation = one_violation(&ROW.replace("unit = \"ms\"", "unit = \"seconds\""));
    assert_eq!(violation.line, 5);
    assert!(violation.message.starts_with("budgets.toml is not valid: "));
    assert!(violation.message.contains("seconds"), "{violation:?}");
}

#[test]
fn a_gate_and_a_status_outside_their_sets_are_reported() {
    let gate = one_violation(&ROW.replace("gate = \"release\"", "gate = \"nightly\""));
    assert_eq!(gate.line, 6);
    assert!(gate.message.contains("nightly"), "{gate:?}");
    let status = one_violation(&ROW.replace("status = \"unmeasured\"", "status = \"done\""));
    assert_eq!(status.line, 8);
    assert!(status.message.contains("done"), "{status:?}");
}

#[test]
fn a_string_where_a_number_goes_is_one_violation_at_its_line() {
    let violation = one_violation(&ROW.replace("limit = 1500.0", "limit = \"1500\""));
    assert_eq!(violation.line, 4);
    assert!(violation.message.starts_with("budgets.toml is not valid: "));
}

#[test]
fn a_number_where_a_string_goes_is_one_violation() {
    let violation =
        one_violation(&ROW.replace("metric = \"cold start to interactive\"", "metric = 4"));
    assert_eq!(violation.line, 3);
    assert!(violation.message.starts_with("budgets.toml is not valid: "));
}

#[test]
fn a_negative_or_infinite_limit_is_reported() {
    let negative = one_violation(&ROW.replace("limit = 1500.0", "limit = -1.0"));
    assert_eq!(negative.line, 4);
    assert_eq!(
        negative.message,
        "budget cold_start: limit must be zero or more"
    );
    let infinite = one_violation(&ROW.replace("limit = 1500.0", "limit = inf"));
    assert_eq!(
        infinite.message,
        "budget cold_start: limit must be zero or more"
    );
}

#[test]
fn a_key_given_twice_is_one_violation_at_the_second_line() {
    let text = ROW.replace(
        "source = \"synthesis §4\"\n",
        "source = \"a\"\nsource = \"b\"\n",
    );
    let violation = one_violation(&text);
    assert_eq!(violation.line, 10);
    assert!(violation.message.starts_with("budgets.toml is not valid: "));
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
fn a_key_before_the_first_header_is_one_violation() {
    let text = format!("id = \"x\"\n{ROW}");
    let violation = one_violation(&text);
    assert_eq!(violation.line, 1);
    assert!(violation.message.starts_with("budgets.toml is not valid: "));
}

#[test]
fn a_file_without_tables_is_reported_on_line_1() {
    let violations = only_violations("# just a comment\n");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].line, 1);
    assert_eq!(violations[0].message, "no [[budget]] tables");
}

#[test]
fn the_header_line_is_the_row_header_above_its_first_key() {
    let text = format!("# the plan\n\n{}", ROW.replace("gate = \"release\"\n", ""));
    let violation = one_violation(&text);
    assert_eq!(violation.line, 3, "{violation:?}");
    assert_eq!(violation.message, "budget cold_start has no gate");
}

#[test]
fn the_header_line_skips_comments_and_blank_lines_below_the_header() {
    let text = "[[budget]]\n# a note\n\n   # indented note\nid = \"a_b\"\nmetric = \"m\"\nlimit = 0\nunit = \"count\"\nmeasured_by = \"x\"\nstatus = \"unmeasured\"\nsource = \"s\"\n";
    let violation = one_violation(text);
    assert_eq!(violation.line, 1, "{violation:?}");
    assert_eq!(violation.message, "budget a_b has no gate");
}

#[test]
fn the_header_line_of_a_row_after_others_is_its_own_header() {
    let text = format!(
        "{ROW}\n# next\n\n{}",
        ROW.replace("cold_start", "warm_start")
            .replace("gate = \"release\"\n", "")
    );
    let Read {
        budgets,
        violations,
        ..
    } = parse_counted(&text);
    assert_eq!(budgets.len(), 1, "the first row is still a budget");
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].line, 13, "{violations:?}");
    assert_eq!(violations[0].message, "budget warm_start has no gate");
}

#[test]
fn a_row_line_is_its_header_whatever_lies_above_it() {
    // The first row's metric is a multi-line string with a line that starts with `[[`. The second
    // row's header has comments and blank lines below it, and is written `[[ budget ]]`.
    let first = ROW.replace(
        "metric = \"cold start to interactive\"",
        "metric = '''\n[[ fake ]]\ncold start\n'''",
    );
    let second = "\n# a note\n\n[[ budget ]]\n   # indented\n\nid = \"b_c\"\nmetric = \"m\"\nlimit = 1\nunit = \"count\"\nmeasured_by = \"x\"\nstatus = \"unmeasured\"\nsource = \"s\"\n";
    let text = format!("{first}{second}");
    let header = text
        .lines()
        .position(|line| line == "[[ budget ]]")
        .map_or(0, |index| index + 1);
    let Read {
        budgets,
        violations,
        ..
    } = parse_counted(&text);
    assert_eq!(budgets.len(), 1, "the first row is still a budget");
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert_eq!(violations[0].line, u32::try_from(header).unwrap_or(0));
    assert_eq!(violations[0].message, "budget b_c has no gate");
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
