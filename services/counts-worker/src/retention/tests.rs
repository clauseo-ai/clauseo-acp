//! The run's deletes against an in-memory stand-in for the four tables, which does what each
//! statement says: the cut-offs, the ranges covering every id, and a backlog larger than any one
//! range cleared in one run.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_counts::schema::Day;

use super::*;

/// The four tables, as their keys and the column each delete compares.
#[derive(Default)]
struct Tables {
    rates: BTreeSet<String>,
    tallies: BTreeMap<String, String>,
    reports: BTreeSet<(String, String)>,
    install_weeks: BTreeSet<(String, String)>,
    crashes: BTreeSet<(String, String)>,
}

fn text(param: &Param) -> &str {
    match param {
        Param::Text(text) => text,
        other => panic!("not text: {other:?}"),
    }
}

impl Tables {
    /// Runs one of the four statements, as SQLite would.
    fn run(&mut self, sql: &str, values: &[Param]) {
        match sql {
            DELETE_RATES => {
                let today = text(&values[0]).to_owned();
                self.rates.retain(|day| *day >= today);
            }
            DELETE_TALLIES => {
                let (low, high, cut) = (text(&values[0]), text(&values[1]), text(&values[2]));
                self.tallies.retain(|id, week| {
                    !(id.as_str() >= low && id.as_str() < high && week.as_str() < cut)
                });
            }
            DELETE_REPORTS => {
                let (low, high, cut) = (text(&values[0]), text(&values[1]), text(&values[2]));
                self.reports.retain(|(install, day)| {
                    !(install.as_str() >= low && install.as_str() < high && day.as_str() < cut)
                });
            }
            DELETE_STATUS | DELETE_DAY_CELLS => {}
            DELETE_INSTALL_WEEKS => {
                let cut = text(&values[0]).to_owned();
                self.install_weeks.retain(|(_, week)| *week >= cut);
            }
            DELETE_CRASHES => {
                let cut = text(&values[0]).to_owned();
                self.crashes.retain(|(_, week)| *week >= cut);
            }
            other => panic!("unknown statement {other}"),
        }
    }

    /// One cron run at `now`; the number of statements it sent.
    fn prune(&mut self, now: Time) -> usize {
        let statements = deletes(now).unwrap();
        for (sql, values) in &statements {
            self.run(sql, values);
        }
        statements.len()
    }
}

/// Monday 2026-11-09, 03:00 UTC.
fn monday() -> Time {
    Day::from_ymd(2026, 11, 9).unwrap().start().plus(3 * 3_600)
}

/// A lowercase v4-shaped id whose first hex digit is `n % 16`.
fn id(n: usize) -> String {
    format!("{:x}{n:07x}-0000-4000-8000-000000000000", n % 16)
}

#[test]
fn the_cut_offs_are_today_90_days_and_396_days_back() {
    let statements = deletes(monday()).unwrap();
    assert_eq!(statements.len(), STATEMENTS);
    assert_eq!(STATEMENTS, 37);
    assert_eq!(
        statements[0],
        (DELETE_RATES, vec![Param::Text("2026-11-09".to_owned())])
    );
    // 2025-10-09 is a Thursday in ISO week 41 of 2025; 90 days before is 2026-08-11.
    for (sql, values) in &statements[1..] {
        let cut = text(values.last().unwrap());
        match *sql {
            DELETE_TALLIES | DELETE_INSTALL_WEEKS => assert_eq!(cut, "2025-W41"),
            DELETE_REPORTS | DELETE_STATUS | DELETE_DAY_CELLS => assert_eq!(cut, "2026-08-11"),
            // 365 days before is Sunday 2025-11-09, in ISO week 45.
            DELETE_CRASHES => assert_eq!(cut, "2025-W45"),
            other => panic!("unexpected {other}"),
        }
    }
    let cut: Day = "2026-08-11".parse().unwrap();
    assert_eq!(
        Day::from_ymd(2026, 11, 9).unwrap().days_since(cut),
        REPORT_DAYS
    );
}

#[test]
fn the_ranges_cover_every_lowercase_id_once_and_return_nothing() {
    let statements = deletes(monday()).unwrap();
    for sql in [DELETE_TALLIES, DELETE_REPORTS] {
        assert!(
            !sql.contains("RETURNING") && !sql.contains("LIMIT"),
            "{sql}"
        );
        let ranges: Vec<(&str, &str)> = statements
            .iter()
            .filter(|(s, _)| *s == sql)
            .map(|(_, values)| (text(&values[0]), text(&values[1])))
            .collect();
        assert_eq!(ranges.len(), 16);
        for id in [
            "00000000-0000-4000-8000-000000000000",
            "9fffffff-ffff-4fff-bfff-ffffffffffff",
            "a0000000-0000-4000-8000-000000000000",
            "ffffffff-ffff-4fff-bfff-ffffffffffff",
            "c0ffee00-0000-4000-8000-000000000001",
        ] {
            let holding = ranges
                .iter()
                .filter(|(low, high)| id >= *low && id < *high)
                .count();
            assert_eq!(holding, 1, "{id} in {sql}");
        }
    }
}

#[test]
fn a_backlog_larger_than_any_range_is_cleared_in_one_run_and_kept_rows_stay() {
    let mut tables = Tables::default();
    tables
        .rates
        .extend(["2026-11-08", "2026-11-09"].map(str::to_owned));
    for n in 0..45_000 {
        tables.reports.insert((id(n), "2026-07-01".to_owned()));
    }
    for n in 0..2_500 {
        tables.reports.insert((id(n), "2026-08-11".to_owned()));
        tables.tallies.insert(id(n), "2025-W40".to_owned());
    }
    tables.tallies.insert(id(99_999), "2025-W41".to_owned());
    for (n, week) in ["2025-W40", "2025-W41", "2026-W45"].into_iter().enumerate() {
        tables.install_weeks.insert((id(n), week.to_owned()));
    }

    for (n, week) in ["2025-W44", "2025-W45", "2026-W45"].into_iter().enumerate() {
        tables.crashes.insert((id(n), week.to_owned()));
    }

    assert_eq!(tables.prune(monday()), 37);
    let crash_weeks: Vec<&str> = tables
        .crashes
        .iter()
        .map(|(_, week)| week.as_str())
        .collect();
    assert_eq!(crash_weeks, ["2025-W45", "2026-W45"]);
    let weeks: Vec<&str> = tables
        .install_weeks
        .iter()
        .map(|(_, week)| week.as_str())
        .collect();
    assert_eq!(weeks, ["2025-W41", "2026-W45"]);
    assert_eq!(tables.rates, BTreeSet::from(["2026-11-09".to_owned()]));
    assert_eq!(tables.tallies.into_keys().collect::<Vec<_>>(), [id(99_999)]);
    assert_eq!(tables.reports.len(), 2_500);
    assert!(tables.reports.iter().all(|(_, day)| day == "2026-08-11"));
}

#[test]
fn a_run_deletes_first_then_fills_within_the_budget() {
    let statements = plan(monday()).unwrap();
    assert_eq!(statements.len(), PLAN);
    assert_eq!(PLAN, 42);
    assert_eq!(statements[..STATEMENTS], deletes(monday()).unwrap()[..]);
    assert_eq!(
        statements[STATEMENTS..],
        fills(monday().day().unwrap()).unwrap()[..]
    );
    assert!(
        statements[..STATEMENTS]
            .iter()
            .all(|(sql, _)| sql.starts_with("DELETE"))
    );
    let steps: Vec<Step> = statements.iter().map(|(sql, _)| Step::of(sql)).collect();
    assert_eq!(
        steps[STATEMENTS..],
        [
            Step::FillWeek,
            Step::FillMonth,
            Step::FillDay,
            Step::FillActive,
            Step::FillActive
        ]
    );
    assert!(steps[..STATEMENTS].iter().all(|step| *step == Step::Delete));
}

#[test]
fn a_delete_never_takes_a_row_a_fill_could_still_use() {
    // On every day for three years, the oldest day a fill reads is the report cut-off or later:
    // a row deleted that day (older than the cut-off) belongs to no period still fillable then
    // or later, as the fill ranges only move forward.
    let start = Day::from_ymd(2026, 1, 1).unwrap();
    for n in 0..1_100 {
        let today = start.days_before(-n).unwrap();
        let now = today.start().plus(3 * 3_600);
        let report_cut: Day = deletes(now)
            .unwrap()
            .iter()
            .find(|(sql, _)| *sql == DELETE_REPORTS)
            .map(|(_, values)| text(&values[2]).parse().unwrap())
            .unwrap();
        // CLEAR_ACTIVE and FILL_ACTIVE take no range: they read only what the deletes kept.
        for (_, values) in fills(today).unwrap().iter().filter(|(_, v)| !v.is_empty()) {
            let first: Day = text(&values[0]).parse().unwrap();
            assert!(first >= report_cut, "{today}: a fill reads from {first}");
        }
    }
}

#[test]
fn the_health_row_counts_each_step_once_per_run() {
    let ok: Vec<(Step, bool)> = Step::ALL.iter().map(|step| (*step, true)).collect();
    let expected = |flags: [i32; 5]| {
        let mut values = vec![Param::Text("2026-11-09".to_owned())];
        for ok in flags {
            values.extend([Param::Int(ok), Param::Int(1 - ok)]);
        }
        values
    };
    assert_eq!(status(monday(), &ok).unwrap(), expected([1, 1, 1, 1, 1]));
    // One failed range fails the delete step; a failed fill fails only its own step.
    let mixed = [
        (Step::Delete, true),
        (Step::Delete, false),
        (Step::FillWeek, false),
        (Step::FillMonth, true),
        (Step::FillDay, true),
        (Step::FillActive, true),
        (Step::FillActive, false),
    ];
    assert_eq!(status(monday(), &mixed).unwrap(), expected([0, 0, 1, 1, 0]));
    // The step names in the statement, in Step::ALL's order, and its eleven values.
    let mut at = 0;
    for step in Step::ALL {
        let found = RECORD_STATUS[at..]
            .find(&format!("('{}', ?1,", step.name()))
            .unwrap();
        at += found;
    }
    assert!(RECORD_STATUS.contains("?11") && !RECORD_STATUS.contains("?12"));
    assert!(!RECORD_STATUS.contains("install") && !RECORD_STATUS.contains("tally"));
}
