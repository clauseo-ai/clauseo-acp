//! The run's plan against an in-memory stand-in for the three tables, which does what each
//! statement says: the cut-offs, a backlog cleared in chunks within a run, and one larger than a
//! run cleared over the next run.

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
    fn run(&mut self, sql: &str, values: &[Param]) -> Vec<String> {
        match sql {
            DELETE_RATES => {
                let today = text(&values[0]).to_owned();
                self.rates.retain(|day| *day >= today);
                Vec::new()
            }
            DELETE_TALLIES => {
                let (cursor, cut) = (text(&values[0]), text(&values[1]));
                let chunk: Vec<String> = self
                    .tallies
                    .iter()
                    .filter(|(id, week)| id.as_str() > cursor && week.as_str() < cut)
                    .map(|(id, _)| id.clone())
                    .take(CHUNK)
                    .collect();
                for id in &chunk {
                    self.tallies.remove(id);
                }
                chunk
            }
            DELETE_REPORTS => {
                let (cursor, cut) = (text(&values[0]), text(&values[1]));
                let chunk: Vec<(String, String)> = self
                    .reports
                    .iter()
                    .filter(|(install, day)| install.as_str() >= cursor && day.as_str() < cut)
                    .take(CHUNK)
                    .cloned()
                    .collect();
                for key in &chunk {
                    self.reports.remove(key);
                }
                chunk.into_iter().map(|(install, _)| install).collect()
            }
            other => panic!("unknown statement {other}"),
        }
    }

    /// One cron run at `now`; the number of statements it sent.
    fn prune(&mut self, now: Time) -> usize {
        let mut pruner = Pruner::new(now).unwrap();
        let mut sent = 0;
        while let Some((sql, values)) = pruner.next() {
            let keys = self.run(sql, &values);
            pruner.record(&keys);
            sent += 1;
        }
        sent
    }
}

/// Monday 2026-11-09, 03:00 UTC.
fn monday() -> Time {
    Day::from_ymd(2026, 11, 9).unwrap().start().plus(3 * 3_600)
}

fn id(n: usize) -> String {
    format!("{n:08x}-0000-4000-8000-000000000000")
}

#[test]
fn the_cut_offs_are_today_90_days_and_396_days_back() {
    let pruner = Pruner::new(monday()).unwrap();
    assert_eq!(
        pruner.next(),
        Some((DELETE_RATES, vec![Param::Text("2026-11-09".to_owned())]))
    );
    assert_eq!(pruner.report_cut, "2026-08-11");
    // 2025-10-09 is a Thursday in ISO week 41 of 2025.
    assert_eq!(pruner.tally_cut, "2025-W41");
    let today = Day::from_ymd(2026, 11, 9).unwrap();
    let cut: Day = pruner.report_cut.parse().unwrap();
    assert_eq!(today.days_since(cut), REPORT_DAYS);
}

#[test]
fn the_statements_chunk_by_the_constant() {
    for sql in [DELETE_TALLIES, DELETE_REPORTS] {
        assert!(sql.contains(&format!("LIMIT {CHUNK}")), "{sql}");
    }
}

#[test]
fn a_backlog_of_several_chunks_is_cleared_in_one_run_and_kept_rows_stay() {
    let mut tables = Tables::default();
    tables
        .rates
        .extend(["2026-11-08", "2026-11-09"].map(str::to_owned));
    for n in 0..2_500 {
        tables.reports.insert((id(n), "2026-08-01".to_owned()));
        tables.reports.insert((id(n), "2026-08-11".to_owned()));
    }
    // One install with more old rows than a chunk, so the cursor stays on it.
    for day in 0..1_200 {
        let old = Day::from_ymd(2026, 8, 10)
            .unwrap()
            .days_before(day)
            .unwrap();
        tables.reports.insert((id(9_999), old.to_string()));
    }
    for n in 0..1_500 {
        tables.tallies.insert(id(n), "2025-W40".to_owned());
    }
    tables.tallies.insert(id(5_000), "2025-W41".to_owned());

    // 1 rates, 2 tally chunks (1,000 and 500), 4 report chunks (3,700 rows).
    assert_eq!(tables.prune(monday()), 7);
    assert_eq!(tables.rates, BTreeSet::from(["2026-11-09".to_owned()]));
    assert_eq!(tables.tallies.into_keys().collect::<Vec<_>>(), [id(5_000)]);
    assert_eq!(tables.reports.len(), 2_500);
    assert!(tables.reports.iter().all(|(_, day)| day == "2026-08-11"));
}

#[test]
fn a_backlog_larger_than_a_run_is_cleared_over_the_next_runs() {
    let mut tables = Tables::default();
    for n in 0..45_000 {
        tables.reports.insert((id(n), "2026-07-01".to_owned()));
    }
    tables.reports.insert((id(0), "2026-11-01".to_owned()));

    assert_eq!(tables.prune(monday()), MAX_STATEMENTS);
    // 1 rates statement, 1 empty tally chunk, 38 report chunks.
    assert_eq!(tables.reports.len(), 45_001 - 38 * CHUNK);
    let next_day = monday().plus(86_400);
    assert_eq!(tables.prune(next_day), 1 + 1 + 8);
    assert_eq!(
        tables.reports,
        BTreeSet::from([(id(0), "2026-11-01".to_owned())])
    );
}
