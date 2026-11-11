//! The run's deletes against an in-memory stand-in for the three tables, which does what each
//! statement says: the cut-offs, the ranges covering every id, and a backlog larger than any one
//! range cleared in one run.

use std::collections::{BTreeMap, BTreeSet};

use clauseo_counts::schema::Day;

use super::*;

/// The three tables, as their keys and the column each delete compares.
#[derive(Default)]
struct Tables {
    rates: BTreeSet<String>,
    tallies: BTreeMap<String, String>,
    reports: BTreeSet<(String, String)>,
}

fn text(param: &Param) -> &str {
    match param {
        Param::Text(text) => text,
        other => panic!("not text: {other:?}"),
    }
}

impl Tables {
    /// Runs one of the three statements, as SQLite would.
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
    assert_eq!(STATEMENTS, 33);
    assert_eq!(
        statements[0],
        (DELETE_RATES, vec![Param::Text("2026-11-09".to_owned())])
    );
    // 2025-10-09 is a Thursday in ISO week 41 of 2025; 90 days before is 2026-08-11.
    for (sql, values) in &statements[1..] {
        let cut = text(&values[2]);
        match *sql {
            DELETE_TALLIES => assert_eq!(cut, "2025-W41"),
            DELETE_REPORTS => assert_eq!(cut, "2026-08-11"),
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

    assert_eq!(tables.prune(monday()), 33);
    assert_eq!(tables.rates, BTreeSet::from(["2026-11-09".to_owned()]));
    assert_eq!(tables.tallies.into_keys().collect::<Vec<_>>(), [id(99_999)]);
    assert_eq!(tables.reports.len(), 2_500);
    assert!(tables.reports.iter().all(|(_, day)| day == "2026-08-11"));
}
