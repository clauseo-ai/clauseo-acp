//! The daily cron (03:00 UTC): first the deletes, then the fills of `install_week`, `rollup`,
//! `day_cell` and `active` (`rollup.rs`), then its own health in `cron_status`. The deletes take
//! `rate` rows of days before today, `tally` and `install_week` rows whose week is more than 396
//! days old, `crash` rows whose week is more than 365 days old, and `report`, `day_cell` and
//! `cron_status` rows whose day is more than 90 days old.
//! `rollup` is kept. The cut-offs come from the Worker's clock through the crate's own `Day`
//! and `Week`.
//!
//! The deletes go first so that the fills' scans can never spend D1's daily reads before them
//! (decision 0061). That loses nothing: a report deleted today is older than 90 days, so every
//! week or month it belongs to began before the first day a fill still takes, and its last day
//! to be filled was yesterday at the latest (`rollup.rs`).
//!
//! Workers Free allows 50 D1 queries and 10 ms of CPU per run, so the deletes are split into
//! fixed ranges of the key's first hex digit (`0` to `f`; the crate stores ids as lowercase
//! UUID v4, which spread evenly over them): 16 for `report`, 16 for `tally`, and one each for
//! `rate`, `install_week`, `crash`, `day_cell` and `cron_status`, 37 statements; 42 with the
//! five fills, 43 with the health row. No statement returns rows, so the Worker decodes nothing
//! per row, and a run clears any backlog D1 can delete within its statement limits.

use clauseo_counts::schema::Time;

use crate::rollup::{
    CLEAR_ACTIVE, FILL_ACTIVE, FILL_DAY_CELL, FILL_INSTALL_WEEK, FILL_ROLLUP, FILLS, fills,
};
use crate::rows::Param;

/// How many days a `report` row is kept past its day.
pub(crate) const REPORT_DAYS: i64 = 90;
/// How many days a `tally` or `install_week` row is kept, counted to the Monday of its week.
pub(crate) const TALLY_DAYS: i64 = 396;
/// How many days a `crash` row is kept, counted to the Monday of its week.
pub(crate) const CRASH_DAYS: i64 = 365;

/// The lower bound of each range, and after the last, the upper bound of the last: lowercase
/// hex in ASCII order. `g` follows `f`, so every id that starts with `f` is below it.
const BOUNDS: [&str; 17] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g",
];

/// Deletes the daily counts of days before today; a handful of rows.
const DELETE_RATES: &str = "DELETE FROM rate WHERE day < ?1";
/// Deletes the tallies in one id range whose week is before the cut-off week. `YYYY-Www` sorts
/// as it reads.
const DELETE_TALLIES: &str = "DELETE FROM tally WHERE tally >= ?1 AND tally < ?2 AND week < ?3";
/// Deletes the install weeks before the cut-off week, in one statement: the table holds one row
/// per install and active week, so about one row per install expires each week.
const DELETE_INSTALL_WEEKS: &str = "DELETE FROM install_week WHERE week < ?1";
/// Deletes the crash rows before the cut-off week, in one statement: at most 200 an hour come in.
const DELETE_CRASHES: &str = "DELETE FROM crash WHERE week < ?1";
/// Deletes `day_cell`'s days before the cut-off day, as `report`'s go.
const DELETE_DAY_CELLS: &str = "DELETE FROM day_cell WHERE day < ?1";
/// Deletes the cron's health rows before the cut-off day.
const DELETE_STATUS: &str = "DELETE FROM cron_status WHERE day < ?1";
/// Deletes the reports in one install range whose day is before the cut-off day.
const DELETE_REPORTS: &str = "DELETE FROM report WHERE install >= ?1 AND install < ?2 AND day < ?3";

/// Adds one run's outcome per step to the day's health rows: `?1` is the day, then for each step
/// of [`Step::ALL`], in order, 1 or 0 finished and 1 or 0 failed.
pub(crate) const RECORD_STATUS: &str = "INSERT INTO cron_status (step, day, ok, failed) VALUES \
     ('delete', ?1, ?2, ?3), ('fill_week', ?1, ?4, ?5), ('fill_month', ?1, ?6, ?7), \
     ('fill_day', ?1, ?8, ?9), ('fill_active', ?1, ?10, ?11) \
     ON CONFLICT (step, day) DO UPDATE SET ok = ok + excluded.ok, failed = failed + excluded.failed";

/// How many deletes a run sends.
pub(crate) const STATEMENTS: usize = 5 + 2 * (BOUNDS.len() - 1);
/// The statements of a run before its health row; Free allows 50 D1 queries an invocation.
pub(crate) const PLAN: usize = STATEMENTS + FILLS;
const _: () = assert!(
    PLAN < 45,
    "with the health row, within 45 of Free's 50 queries"
);

/// One step of the cron, as `cron_status` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Every delete.
    Delete,
    /// The `install_week` fill.
    FillWeek,
    /// The `rollup` fill.
    FillMonth,
    /// The `day_cell` fill.
    FillDay,
    /// Emptying and refilling `active`.
    FillActive,
}

impl Step {
    /// Every step, in the order [`RECORD_STATUS`] lists them.
    pub(crate) const ALL: [Step; 5] = [
        Step::Delete,
        Step::FillWeek,
        Step::FillMonth,
        Step::FillDay,
        Step::FillActive,
    ];

    /// The name in `cron_status`, which the tests hold [`RECORD_STATUS`] to.
    #[cfg(test)]
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Step::Delete => "delete",
            Step::FillWeek => "fill_week",
            Step::FillMonth => "fill_month",
            Step::FillDay => "fill_day",
            Step::FillActive => "fill_active",
        }
    }

    /// The step a statement of [`plan`] belongs to.
    pub(crate) fn of(sql: &str) -> Step {
        match sql {
            FILL_INSTALL_WEEK => Step::FillWeek,
            FILL_ROLLUP => Step::FillMonth,
            FILL_DAY_CELL => Step::FillDay,
            CLEAR_ACTIVE | FILL_ACTIVE => Step::FillActive,
            _ => Step::Delete,
        }
    }
}

/// A run at `now`: the deletes, then the fills, or `None` when the clock is outside the years
/// a `Day` holds.
pub(crate) fn plan(now: Time) -> Option<Vec<(&'static str, Vec<Param>)>> {
    let mut statements = deletes(now)?;
    statements.extend(fills(now.day()?)?);
    Some(statements)
}

/// The values of [`RECORD_STATUS`] for a run on `now`'s day whose statements had `outcomes`
/// (each statement's step and whether it succeeded): a step finished when all of its statements
/// did.
pub(crate) fn status(now: Time, outcomes: &[(Step, bool)]) -> Option<Vec<Param>> {
    let mut values = vec![Param::Text(now.day()?.to_string())];
    for step in Step::ALL {
        let ok = outcomes
            .iter()
            .filter(|(of, _)| *of == step)
            .all(|(_, ok)| *ok);
        values.push(Param::Int(i32::from(ok)));
        values.push(Param::Int(i32::from(!ok)));
    }
    Some(values)
}

/// Every delete of a run at `now`, in order, or `None` when the clock is outside the years a
/// `Day` holds.
pub(crate) fn deletes(now: Time) -> Option<Vec<(&'static str, Vec<Param>)>> {
    let today = now.day()?;
    let tally_cut = today.days_before(TALLY_DAYS)?.week().to_string();
    let report_cut = today.days_before(REPORT_DAYS)?.to_string();
    let ranges = || BOUNDS.windows(2).map(|pair| (pair[0], pair[1]));
    let text = |value: &str| Param::Text(value.to_owned());
    let mut statements = vec![(DELETE_RATES, vec![Param::Text(today.to_string())])];
    for (sql, cut) in [(DELETE_TALLIES, &tally_cut), (DELETE_REPORTS, &report_cut)] {
        statements
            .extend(ranges().map(|(low, high)| (sql, vec![text(low), text(high), text(cut)])));
    }
    statements.push((DELETE_INSTALL_WEEKS, vec![text(&tally_cut)]));
    let crash_cut = today.days_before(CRASH_DAYS)?.week().to_string();
    statements.push((DELETE_CRASHES, vec![text(&crash_cut)]));
    statements.push((DELETE_DAY_CELLS, vec![text(&report_cut)]));
    statements.push((DELETE_STATUS, vec![text(&report_cut)]));
    Some(statements)
}

#[cfg(test)]
mod tests;
