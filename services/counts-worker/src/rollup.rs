//! The daily cron's fills (decision 0059), which run before its deletes: `install_week`, one row
//! per install per final ISO week, and `rollup`, the monthly totals per version, OS, event and
//! cell with their distinct installs. Both are `INSERT OR IGNORE … SELECT` over `report`, with
//! `json_each` over the cells, so D1 does the adding up and the Worker decodes no row.
//!
//! A day report is accepted up to 14 days after its day (the crate's `MAX_DAY_AGE`), so a week
//! or month is final once its last day is at least 15 days before today: no request can add a
//! row to it any more. A run fills only periods that are final *and* whole: every day of the
//! period is on or after the day the deletes keep from (today less 90 days), so all of its rows
//! still exist. A period stays fillable for many days (a week from 15 to 90 days after its
//! Monday, a month for at least 45 days), so missed runs do not lose it; once filled, a later
//! run inserts nothing new, because the key of every row it could make is already there and
//! the period's rows can only have shrunk. A period that is never filled while whole is never
//! filled from part of its rows.

use clauseo_counts::schema::{Day, MAX_DAY_AGE};

use crate::retention::REPORT_DAYS;
use crate::rows::Param;

/// The ISO week label (`YYYY-Www`, as the crate's `Week` prints it) of a `YYYY-MM-DD` column, in
/// plain SQLite: the week's Thursday gives the ISO year, and its day of the year the number. A
/// macro, so the fill and the migration's views use the same text.
macro_rules! iso_week {
    ($day:literal) => {
        concat!(
            "printf('%s-W%02d', strftime('%Y', date(",
            $day,
            ", (3 - (strftime('%w', ",
            $day,
            ") + 6) % 7) || ' days')), (CAST(strftime('%j', date(",
            $day,
            ", (3 - (strftime('%w', ",
            $day,
            ") + 6) % 7) || ' days')) AS INTEGER) - 1) / 7 + 1)"
        )
    };
}

/// Fills `install_week` from the reports of `?1` to `?2` (whole final weeks). Per install and
/// week: the days reported, the questions sent, whether a limit was reached, and how many
/// distinct features were used. `first_week` is the earliest week already in `install_week`
/// for the install, or the earliest in this run's range. Every report has at least one cell
/// (the crate refuses an empty one), so the join with its cells drops no day.
pub(crate) const FILL_INSTALL_WEEK: &str = concat!(
    "WITH weekly AS (SELECT r.install AS install, ",
    iso_week!("r.day"),
    " AS week, COUNT(DISTINCT r.day) AS days_active, ",
    "COALESCE(SUM(CASE WHEN e.key = 'question_sent' THEN json_extract(c.value, '$.n') END), 0) \
     AS questions, ",
    "MAX(e.key = 'limit' AND json_extract(c.value, '$.kind') = 'reached') AS hit_limit, ",
    "COUNT(DISTINCT CASE WHEN e.key = 'feature_used' THEN json_extract(c.value, '$.feature') \
     END) AS features ",
    "FROM report AS r CROSS JOIN json_each(r.cells) AS e CROSS JOIN json_each(e.value) AS c ",
    "WHERE r.day BETWEEN ?1 AND ?2 GROUP BY r.install, week) ",
    "INSERT OR IGNORE INTO install_week \
     (install, week, first_week, days_active, questions, hit_limit, features) ",
    "SELECT install, week, min(coalesce((SELECT MIN(p.first_week) FROM install_week AS p \
     WHERE p.install = weekly.install), week), MIN(week) OVER (PARTITION BY install)), \
     days_active, questions, hit_limit, features FROM weekly"
);

/// Fills `rollup` from the reports of `?1` to `?2` (whole final months): per month, version,
/// OS, event and cell (the cell's fields without `n`), the summed count and the distinct
/// installs, counted now while the rows still carry install ids. The `_report` row of each
/// month, version and OS counts the reports (install-days) and their distinct installs; it
/// rides along as one more cell, so `report` is read once.
pub(crate) const FILL_ROLLUP: &str = "WITH cells AS (\
     SELECT substr(r.day, 1, 7) AS month, r.version AS version, r.os AS os, \
     r.install AS install, e.key AS event, json_remove(c.value, '$.n') AS cell, \
     json_extract(c.value, '$.n') AS n \
     FROM report AS r \
     CROSS JOIN json_each(json_set(r.cells, '$._report', json('[{\"n\":1}]'))) AS e \
     CROSS JOIN json_each(e.value) AS c \
     WHERE r.day BETWEEN ?1 AND ?2) \
     INSERT OR IGNORE INTO rollup (month, version, os, agent, event, cell, count, installs) \
     SELECT month, version, os, json_extract(cell, '$.agent'), event, cell, SUM(n), \
     COUNT(DISTINCT install) FROM cells GROUP BY month, version, os, event, cell";

/// How many statements [`fills`] sends.
pub(crate) const FILLS: usize = 2;

/// The last day no request can add a report for: 15 days before today.
fn last_final(today: Day) -> Option<Day> {
    today.days_before(MAX_DAY_AGE + 1)
}

/// The first day whose reports all survive this run's deletes.
fn first_kept(today: Day) -> Option<Day> {
    today.days_before(REPORT_DAYS)
}

fn next_day(day: Day) -> Option<Day> {
    day.days_before(-1)
}

/// The year, month and day of `day`, from the form it prints in.
fn ymd(day: Day) -> Option<(i64, i64, i64)> {
    let text = day.to_string();
    let mut parts = text.split('-').map(str::parse::<i64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ))
}

/// The first day of the month `day` falls in.
fn month_start(day: Day) -> Option<Day> {
    let (year, month, _) = ymd(day)?;
    Day::from_ymd(year, month, 1)
}

/// The first and last day of the whole final weeks a run at `today` fills: Monday to Sunday.
/// The first may come after the last, and then the range is empty. `None` only when the clock
/// is outside the years a `Day` holds.
pub(crate) fn week_span(today: Day) -> Option<(Day, Day)> {
    let kept = first_kept(today)?;
    let final_day = last_final(today)?;
    let mut first = kept.week().monday();
    if first < kept {
        first = kept.week().next().monday();
    }
    let monday = final_day.week().monday();
    let sunday = monday.days_before(-6)?;
    let last = if sunday <= final_day {
        sunday
    } else {
        monday.days_before(1)?
    };
    Some((first, last))
}

/// The first and last day of the whole final months a run at `today` fills, as
/// [`week_span`].
pub(crate) fn month_span(today: Day) -> Option<(Day, Day)> {
    let kept = first_kept(today)?;
    let final_day = last_final(today)?;
    let start = month_start(kept)?;
    let first = if start == kept {
        kept
    } else {
        // 31 days after a month's first day is always in the next month.
        month_start(start.days_before(-31)?)?
    };
    let last = month_start(next_day(final_day)?)?.days_before(1)?;
    Some((first, last))
}

/// The fills of a run at `today`, in order, or `None` when the clock is outside the years a
/// `Day` holds. An empty range still sends its statement, which then inserts nothing.
pub(crate) fn fills(today: Day) -> Option<Vec<(&'static str, Vec<Param>)>> {
    let range = |(first, last): (Day, Day)| {
        vec![
            Param::Text(first.to_string()),
            Param::Text(last.to_string()),
        ]
    };
    Some(vec![
        (FILL_INSTALL_WEEK, range(week_span(today)?)),
        (FILL_ROLLUP, range(month_span(today)?)),
    ])
}

#[cfg(test)]
mod tests;
