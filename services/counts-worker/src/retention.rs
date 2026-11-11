//! The daily cron's deletes (03:00 UTC): `rate` rows of days before today, `tally` rows whose
//! week is more than 396 days old, and `report` rows whose day is more than 90 days old. The
//! cut-offs come from the Worker's clock through the crate's own `Day` and `Week`.
//!
//! Workers Free allows 50 D1 queries and 10 ms of CPU per run, so the deletes are split into
//! fixed ranges of the key's first hex digit (`0` to `f`; the crate stores ids as lowercase
//! UUID v4, which spread evenly over them): 16 for `report`, 16 for `tally` and one for `rate`,
//! 33 statements in all. No statement returns rows, so the Worker decodes nothing per row, and a
//! run clears any backlog D1 can delete within its statement limits.

use clauseo_counts::schema::Time;

use crate::rows::Param;

/// How many days a `report` row is kept past its day.
pub(crate) const REPORT_DAYS: i64 = 90;
/// How many days a `tally` row is kept, counted to the Monday of its week.
pub(crate) const TALLY_DAYS: i64 = 396;

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
/// Deletes the reports in one install range whose day is before the cut-off day.
const DELETE_REPORTS: &str = "DELETE FROM report WHERE install >= ?1 AND install < ?2 AND day < ?3";

/// The most statements a run sends; Free allows 50 D1 queries an invocation.
pub(crate) const STATEMENTS: usize = 1 + 2 * (BOUNDS.len() - 1);
const _: () = assert!(
    STATEMENTS <= 40,
    "within the 40-statement budget under Free's 50"
);

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
    Some(statements)
}

#[cfg(test)]
mod tests;
