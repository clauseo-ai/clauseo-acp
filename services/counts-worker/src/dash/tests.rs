//! The page from fixed rows: six charts, no script or outside asset, text escaped; and the
//! statements it runs, which read only the views.

use serde_json::json;

use super::*;
use crate::query::guard;

fn data() -> Data {
    Data {
        active: vec![
            json!({ "period": "day", "start": "2026-11-01", "installs": 7 }),
            json!({ "period": "day", "start": "2026-11-02", "installs": 9 }),
            json!({ "period": "week", "start": "2026-W44", "installs": 12 }),
            json!({ "period": "month", "start": "2026-11", "installs": 15 }),
        ],
        cohorts: vec![
            json!({ "first_week": "2026-W40", "week": "2026-W40", "installs": 10 }),
            json!({ "first_week": "2026-W40", "week": "2026-W42", "installs": 6 }),
        ],
        daily: vec![
            json!({ "event": "_report", "bucket": null, "on_day": "2026-11-01", "count": 7, "installs": 7 }),
            json!({ "event": "question_sent", "bucket": "claude", "on_day": "2026-11-01", "count": 21, "installs": 7 }),
            json!({ "event": "answer_ended", "bucket": "lt_10s", "on_day": null, "count": 40, "installs": 6 }),
            json!({ "event": "answer_ended", "bucket": "lt_1s", "on_day": null, "count": 5, "installs": 5 }),
            json!({ "event": "onboarding_step", "bucket": "welcome_shown", "on_day": null, "count": 9, "installs": 9 }),
        ],
        tally: vec![
            json!({ "week": "2026-W44", "version": "3.0.4", "event": "_tally", "count": 20 }),
            json!({ "week": "2026-W44", "version": "3.0.4", "event": "_crashed", "count": 5 }),
            json!({ "week": "2026-W45", "version": "3.0.4", "event": "_tally", "count": 10 }),
            json!({ "week": "2026-W45", "version": "<b>", "event": "_tally", "count": 8 }),
        ],
        unfilled: vec![],
        cron: vec![
            json!({ "step": "delete", "day": "2026-11-01", "ok": 1, "failed": 0 }),
            json!({ "step": "fill_week", "day": "2026-11-01", "ok": 1, "failed": 0 }),
        ],
    }
}

#[test]
fn the_page_has_its_six_charts_and_nothing_from_elsewhere() {
    let page = page(&data());
    assert_eq!(page.matches("<h2>").count(), 7);
    let lower = page.to_lowercase();
    for banned in [
        "<script", "src=", "href=", "url(", "@import", "<link", "http", " onload=",
    ] {
        assert!(!lower.contains(banned), "{banned}");
    }
    assert!(
        page.contains("&lt;b&gt;") && !page.contains("<b>"),
        "escaped"
    );
    // 15 of 20 crash-free; 10 tallies with the crashed hidden: at least 60%.
    assert!(page.contains("<td>75%</td>"));
    assert!(page.contains("<td>&ge;60%</td>"));
    // The answer-time bars come in the crate's order: under 1 s before under 10 s.
    assert!(page.find("lt_1s: 5").unwrap() < page.find("lt_10s: 40").unwrap());
    // Questions per active day: 21 over 7 reports.
    assert!(page.contains("<title>claude</title>"));
    // The cohort's third week shades at 60% of its first.
    assert!(page.contains("rgba(37,99,235,0.60)\">6</td>"));
}

#[test]
fn an_empty_page_says_so() {
    let page = page(&Data::default());
    assert_eq!(page.matches("<h2>").count(), 7);
    assert!(page.contains("No group of 5 or more yet."));
}

#[test]
fn the_charts_read_only_the_views_and_the_health_reads_no_id() {
    let statements = statements();
    assert_eq!(statements.len(), 6);
    for statement in &statements[..4] {
        assert!(guard(statement).is_ok(), "{statement}");
    }
    for statement in &statements {
        assert!(!statement.contains(';'));
    }
    assert!(statements[0].contains("v_active"));
    assert!(statements[1].contains("v_cohort_week"));
    assert!(statements[2].contains("v_daily"));
    assert!(statements[3].contains("v_tally"));
    // The health statements return period labels and counts, never a row of a table with ids.
    assert!(statements[4].starts_with("WITH days AS (SELECT DISTINCT day FROM report"));
    assert!(statements[4].contains("SELECT 'week' AS period, week AS start FROM weeks"));
    assert!(statements[4].contains(crate::rollup::iso_week!("d.day")));
    assert!(statements[5].starts_with("SELECT step, day, ok, failed FROM cron_status"));
}

#[test]
fn the_health_flags_unfilled_periods_and_failed_steps() {
    let mut data = data();
    let quiet = page(&data);
    assert!(quiet.contains("Every final week and month with reports is filled."));
    assert!(quiet.contains("No step failed in the last 30 days."));
    assert!(quiet.contains("Cron runs recorded on 1 of the last 30 days."));
    data.unfilled = vec![
        json!({ "period": "week", "start": "2026-W40" }),
        json!({ "period": "month", "start": "2026-09" }),
    ];
    data.cron
        .push(json!({ "step": "fill_month", "day": "2026-11-02", "ok": 0, "failed": 1 }));
    let loud = page(&data);
    assert!(loud.contains("<strong>Not filled</strong>"));
    assert!(loud.contains(" week 2026-W40; month 2026-09;"));
    assert!(loud.contains("<tr><td>2026-11-02</td><td>fill_month</td><td>1</td><td>0</td></tr>"));
}

#[test]
fn the_policy_is_s23s() {
    assert_eq!(CSP, "default-src 'none'; style-src 'unsafe-inline'");
}
