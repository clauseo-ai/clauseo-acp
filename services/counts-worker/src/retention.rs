//! The daily cron's deletes (03:00 UTC): `rate` rows of days before today, `tally` rows whose
//! week is more than 396 days old, and `report` rows whose day is more than 90 days old. The
//! cut-offs come from the Worker's clock through the crate's own [`Day`] and [`Week`], and the
//! Worker reads no row, only the keys each delete returns.
//!
//! Workers Free allows 50 D1 queries and 10 ms of CPU per run, so the deletes are chunked: at
//! most [`CHUNK`] rows a statement and [`MAX_STATEMENTS`] statements a run. A chunk deletes the
//! first rows past a cursor in key order and moves the cursor to the last key it deleted, so a
//! run scans each table about once. A backlog larger than one run is cleared over the next
//! days' runs. [`Pruner`] is the plan, kept apart from D1 so it can be checked natively.

use clauseo_counts::schema::{Day, Time};

use crate::rows::Param;

/// How many days a `report` row is kept past its day.
pub(crate) const REPORT_DAYS: i64 = 90;
/// How many days a `tally` row is kept, counted to the Monday of its week.
pub(crate) const TALLY_DAYS: i64 = 396;
/// The most rows one delete statement removes.
pub(crate) const CHUNK: usize = 1_000;
/// The most D1 statements one run sends (Free allows 50 queries an invocation).
pub(crate) const MAX_STATEMENTS: usize = 40;
const _: () = assert!(
    MAX_STATEMENTS < 50,
    "Workers Free allows 50 D1 queries a run"
);

/// Deletes the daily counts of days before today; a handful of rows, in one statement.
const DELETE_RATES: &str = "DELETE FROM rate WHERE day < ?1";
/// Deletes up to [`CHUNK`] tallies before the cut-off week, after the cursor, and returns their
/// ids. `YYYY-Www` sorts as it reads.
const DELETE_TALLIES: &str = "DELETE FROM tally WHERE tally IN \
     (SELECT tally FROM tally WHERE tally > ?1 AND week < ?2 ORDER BY tally LIMIT 1000) \
     RETURNING tally";
/// Deletes up to [`CHUNK`] reports before the cut-off day, from the cursor's install on, and
/// returns their install ids. `>=`, as the cursor's install may have more rows left.
const DELETE_REPORTS: &str = "DELETE FROM report WHERE (install, day) IN \
     (SELECT install, day FROM report WHERE install >= ?1 AND day < ?2 \
     ORDER BY install, day LIMIT 1000) RETURNING install";

/// Which table the next statement deletes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Rates,
    Tallies,
    Reports,
    Done,
}

/// One run's deletes: [`Pruner::next`] gives the next statement, [`Pruner::record`] takes the
/// keys it returned.
#[derive(Debug, Clone)]
pub(crate) struct Pruner {
    today: Day,
    tally_cut: String,
    report_cut: String,
    stage: Stage,
    cursor: String,
    sent: usize,
}

impl Pruner {
    /// The run at `now`, or `None` when the clock is outside the years a [`Day`] holds.
    pub(crate) fn new(now: Time) -> Option<Pruner> {
        let today = now.day()?;
        Some(Pruner {
            today,
            tally_cut: today.days_before(TALLY_DAYS)?.week().to_string(),
            report_cut: today.days_before(REPORT_DAYS)?.to_string(),
            stage: Stage::Rates,
            cursor: String::new(),
            sent: 0,
        })
    }

    /// The next statement and its values, or `None` when the run is done or has sent
    /// [`MAX_STATEMENTS`].
    pub(crate) fn next(&self) -> Option<(&'static str, Vec<Param>)> {
        if self.sent >= MAX_STATEMENTS {
            return None;
        }
        let cursor = Param::Text(self.cursor.clone());
        match self.stage {
            Stage::Rates => Some((DELETE_RATES, vec![Param::Text(self.today.to_string())])),
            Stage::Tallies => Some((
                DELETE_TALLIES,
                vec![cursor, Param::Text(self.tally_cut.clone())],
            )),
            Stage::Reports => Some((
                DELETE_REPORTS,
                vec![cursor, Param::Text(self.report_cut.clone())],
            )),
            Stage::Done => None,
        }
    }

    /// The keys the last statement returned: a full chunk moves the cursor to the last of them,
    /// a short one ends its table.
    pub(crate) fn record(&mut self, keys: &[String]) {
        self.sent += 1;
        if self.stage != Stage::Rates
            && keys.len() >= CHUNK
            && let Some(last) = keys.iter().max()
        {
            self.cursor.clone_from(last);
            return;
        }
        self.cursor.clear();
        self.stage = match self.stage {
            Stage::Rates => Stage::Tallies,
            Stage::Tallies => Stage::Reports,
            Stage::Reports | Stage::Done => Stage::Done,
        };
    }
}

#[cfg(test)]
mod tests;
