//! The final-period rule over years of days: every week and month is filled on some day, and
//! only on days when it is final and all of its rows still exist. The SQL's paths into the
//! cells are pinned to the crate's own wire names, and the migration's week label to the fill's.

use std::collections::BTreeSet;

use clauseo_counts::schema::{
    Agent, Cell, Count, DayCells, Delivery, Feature, FeatureUsed, Limit, LimitKind, QuestionSent,
};

use super::*;

fn day(text: &str) -> Day {
    text.parse().unwrap()
}

fn span(range: (Day, Day)) -> (String, String) {
    (range.0.to_string(), range.1.to_string())
}

#[test]
fn the_spans_on_known_days() {
    // Monday 2026-11-09: the last final day is Sunday 2026-10-25 and the first kept day
    // Tuesday 2026-08-11, so the weeks run from Monday 2026-08-17; September is the one month.
    let monday = day("2026-11-09");
    assert_eq!(
        span(week_span(monday).unwrap()),
        ("2026-08-17".to_owned(), "2026-10-25".to_owned())
    );
    assert_eq!(
        span(month_span(monday).unwrap()),
        ("2026-09-01".to_owned(), "2026-09-30".to_owned())
    );
    // October 31 is last accepted on November 14; October is final on the 15th, not before.
    assert_eq!(month_span(day("2026-11-14")).unwrap().1, day("2026-09-30"));
    assert_eq!(month_span(day("2026-11-15")).unwrap().1, day("2026-10-31"));
    // A Sunday is last accepted 14 days later; its week is final the day after that.
    assert_eq!(week_span(day("2026-11-08")).unwrap().1, day("2026-10-18"));
    assert_eq!(week_span(day("2026-11-09")).unwrap().1, day("2026-10-25"));
    // The first kept day on a Monday starts the weeks there; on the 1st, the months.
    assert_eq!(week_span(day("2026-11-15")).unwrap().0, day("2026-08-17"));
    assert_eq!(month_span(day("2026-11-29")).unwrap().0, day("2026-09-01"));
    assert_eq!(month_span(day("2026-11-30")).unwrap().0, day("2026-09-01"));
    assert_eq!(month_span(day("2026-12-01")).unwrap().0, day("2026-10-01"));
}

#[test]
fn every_period_is_filled_on_some_day_and_only_while_final_and_whole() {
    let start = day("2026-01-01");
    let mut weeks = BTreeSet::new();
    let mut months = BTreeSet::new();
    for n in 0..900 {
        let today = start.days_before(-n).unwrap();
        let final_day = today.days_before(15).unwrap();
        let kept = today.days_before(90).unwrap();
        let (first, last) = week_span(today).unwrap();
        assert_eq!(first, first.week().monday(), "{today}");
        assert_eq!(
            last.days_before(6).unwrap(),
            last.week().monday(),
            "{today}"
        );
        assert!(first >= kept && last <= final_day, "{today}");
        // Nothing final and whole is left out: the next week would not be final, the one
        // before not whole.
        assert!(last.days_before(-7).unwrap() > final_day, "{today}");
        assert!(first.days_before(7).unwrap() < kept, "{today}");
        let mut monday = first;
        while monday < last {
            weeks.insert(monday);
            monday = monday.days_before(-7).unwrap();
        }
        let (first, last) = month_span(today).unwrap();
        assert!(first >= kept && last <= final_day, "{today}");
        if first <= last {
            assert_eq!(month_start(first), Some(first), "{today}");
            assert_eq!(
                month_start(next_day(last).unwrap()),
                next_day(last),
                "{today}"
            );
            let mut start = first;
            while start <= last {
                months.insert(start);
                start = month_start(start.days_before(-31).unwrap()).unwrap();
            }
        }
    }
    // Every week and month from the first that could be whole up to the last that is final.
    let mondays: Vec<Day> = weeks.iter().copied().collect();
    for pair in mondays.windows(2) {
        assert_eq!(pair[1].days_since(pair[0]), 7, "a week was never filled");
    }
    assert!(mondays.len() > 120);
    assert_eq!(months.len(), 31, "every month from 2025-11 to 2028-05");
    assert_eq!(months.first(), Some(&day("2025-11-01")));
    assert_eq!(months.last(), Some(&day("2028-05-01")));
}

#[test]
fn the_cells_paths_are_the_crates_wire_names() {
    let cells = DayCells {
        question_sent: vec![Cell {
            key: QuestionSent {
                agent: Agent::Claude,
                delivery: Delivery::Now,
            },
            n: Count::ONE,
        }],
        limit: vec![Cell {
            key: Limit {
                agent: Agent::Claude,
                kind: LimitKind::Reached,
            },
            n: Count::ONE,
        }],
        feature_used: vec![Cell {
            key: FeatureUsed {
                feature: Feature::Compare,
            },
            n: Count::ONE,
        }],
        ..DayCells::default()
    };
    let json = serde_json::to_value(&cells).unwrap();
    assert_eq!(json["question_sent"][0]["n"], 1);
    assert_eq!(json["question_sent"][0]["agent"], "claude");
    assert_eq!(json["limit"][0]["kind"], LimitKind::Reached.as_str());
    assert_eq!(json["feature_used"][0]["feature"], "compare");
    for path in [
        "'question_sent'",
        "'$.n'",
        "'limit'",
        "'$.kind'",
        "'reached'",
        "'feature_used'",
        "'$.feature'",
    ] {
        assert!(FILL_INSTALL_WEEK.contains(path), "{path}");
    }
    assert!(FILL_ROLLUP.contains("'$.agent'") && FILL_ROLLUP.contains("'$.n'"));
}

#[test]
fn the_views_use_the_fills_week_label_and_the_crate_agrees_on_its_edges() {
    let migration = include_str!("../../migrations/0002_rollup.sql");
    assert!(migration.contains(iso_week!("r.day")));
    assert!(FILL_INSTALL_WEEK.contains(iso_week!("r.day")));
    // The days tests/views.test.mjs labels through v_active.
    for (date, week) in [
        ("2026-12-31", "2026-W53"),
        ("2027-01-03", "2026-W53"),
        ("2027-01-04", "2027-W01"),
        ("2024-12-30", "2025-W01"),
        ("2026-11-15", "2026-W46"),
    ] {
        assert_eq!(day(date).week().to_string(), week, "{date}");
    }
}

#[test]
fn a_run_sends_both_fills_with_their_ranges() {
    let statements = fills(day("2026-11-09")).unwrap();
    assert_eq!(statements.len(), FILLS);
    assert_eq!(
        statements[0],
        (
            FILL_INSTALL_WEEK,
            vec![
                Param::Text("2026-08-17".to_owned()),
                Param::Text("2026-10-25".to_owned())
            ]
        )
    );
    assert_eq!(statements[1].0, FILL_ROLLUP);
    for (sql, _) in &statements {
        assert!(
            sql.contains("INSERT OR IGNORE") && !sql.contains("RETURNING"),
            "{sql}"
        );
    }
}
