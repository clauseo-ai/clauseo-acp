//! The daily cron (03:00 UTC): first the fills of `install_week` and `rollup` (`rollup.rs`),
//! then the deletes: `rate` rows of days before today, `tally` and `install_week` rows whose
//! week is more than 396 days old, `crash` rows whose week is more than 365 days old, and
//! `report` rows whose day is more than 90 days old.
//! `rollup` is kept. The cut-offs come from the Worker's clock through the crate's own `Day`
//! and `Week`.
//!
//! Workers Free allows 50 D1 queries and 10 ms of CPU per run, so the deletes are split into
//! fixed ranges of the key's first hex digit (`0` to `f`; the crate stores ids as lowercase
//! UUID v4, which spread evenly over them): 16 for `report`, 16 for `tally`, and one each for
//! `rate`, `install_week` (about one row per install expires each week) and `crash`, 35
//! statements, 37 with the two fills. No statement returns rows, so the Worker decodes nothing
//! per row, and a run clears any backlog D1 can delete within its statement limits.

use clauseo_counts::schema::Time;

use crate::rollup::{FILLS, fills};
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
/// Deletes the reports in one install range whose day is before the cut-off day.
const DELETE_REPORTS: &str = "DELETE FROM report WHERE install >= ?1 AND install < ?2 AND day < ?3";

/// How many deletes a run sends.
pub(crate) const STATEMENTS: usize = 3 + 2 * (BOUNDS.len() - 1);
/// The most statements a run sends, fills and deletes; Free allows 50 D1 queries an invocation.
pub(crate) const PLAN: usize = FILLS + STATEMENTS;
const _: () = assert!(PLAN <= 40, "within the 40-statement budget under Free's 50");

/// A run at `now`: the fills, then the deletes, or `None` when the clock is outside the years
/// a `Day` holds.
pub(crate) fn plan(now: Time) -> Option<Vec<(&'static str, Vec<Param>)>> {
    let mut statements = fills(now.day()?)?;
    statements.extend(deletes(now)?);
    Some(statements)
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
    Some(statements)
}

#[cfg(test)]
mod tests;
