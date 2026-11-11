//! What the Worker stores: one `report` row per day report and one `tally` row per tally, made
//! from the validated value and never from the request's bytes. Each closed top-level field of
//! [`DayReport`] and [`TallyReport`] is a column, written as it is on the wire; `cells` is one
//! JSON column, the cells re-serialised by the crate. `schema` is left out (migration
//! `0001_init.sql` holds schema 1 only), and nothing about the request is kept: no time it
//! arrived, no address, no header.

use clauseo_counts::schema::{DayReport, OsMajor, TallyReport, Time, parse_batch, parse_tally};

use crate::route::{Refusal, Route};

/// One bound value of a statement, kept apart from D1's own type so the rows can be checked
/// natively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Param {
    /// A `TEXT` value.
    Text(String),
    /// An `INTEGER` value.
    Int(i32),
    /// `NULL`.
    Null,
}

/// A day report's row; `(install, day)` is the key, so a day sent again changes nothing.
pub(crate) const INSERT_REPORT: &str = "INSERT OR IGNORE INTO report \
     (install, day, version, channel, os, os_major, arch, language, cells) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";

/// A tally's row; the tally id is the key, so a tally sent again changes nothing.
pub(crate) const INSERT_TALLY: &str = "INSERT OR IGNORE INTO tally \
     (tally, week, version, channel, os, os_major, arch, install_type, managed, cells) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

/// Adds one to the route's count for the UTC day (`?2`) unless it has reached the cap (`?3`),
/// and returns the new count. At the cap the update does not happen and no row comes back, so a
/// refused request writes nothing. Run only for a request that passed every other check, and
/// before its rows.
pub(crate) const BUMP_RATE: &str = "INSERT INTO rate (route, day, n) VALUES (?1, ?2, 1) \
     ON CONFLICT (route, day) DO UPDATE SET n = n + 1 WHERE n < ?3 RETURNING n";

/// A request that passed the schema: the statement and one set of values per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Accepted {
    /// [`INSERT_REPORT`] or [`INSERT_TALLY`].
    pub(crate) insert: &'static str,
    /// 1 to 7 rows for `/v1/counts`, 1 for `/v1/tally`.
    pub(crate) rows: Vec<Vec<Param>>,
}

/// Reads an inflated body through the crate's own parser, with today or this week taken from
/// `now`, and makes its rows. Anything the parser refuses is [`Refusal::Bad`].
pub(crate) fn accept(route: Route, json: &[u8], now: Time) -> Result<Accepted, Refusal> {
    let today = now.day().ok_or(Refusal::Failed)?;
    match route {
        Route::Counts => {
            let batch = parse_batch(json, today)?;
            let rows = batch
                .days
                .iter()
                .map(report_row)
                .collect::<Result<_, _>>()?;
            Ok(Accepted {
                insert: INSERT_REPORT,
                rows,
            })
        }
        Route::Tally => {
            let tally = parse_tally(json, today.week())?;
            Ok(Accepted {
                insert: INSERT_TALLY,
                rows: vec![tally_row(&tally)?],
            })
        }
    }
}

/// The values of [`INSERT_REPORT`] for one day.
fn report_row(day: &DayReport) -> Result<Vec<Param>, Refusal> {
    Ok(vec![
        Param::Text(day.install.to_string()),
        Param::Text(day.day.to_string()),
        Param::Text(day.version.to_string()),
        Param::Text(day.channel.as_str().to_owned()),
        Param::Text(day.os.as_str().to_owned()),
        os_major(day.os_major),
        Param::Text(day.arch.as_str().to_owned()),
        Param::Text(day.language.as_str().to_owned()),
        Param::Text(canonical(&day.cells)?),
    ])
}

/// The values of [`INSERT_TALLY`] for one tally.
fn tally_row(tally: &TallyReport) -> Result<Vec<Param>, Refusal> {
    Ok(vec![
        Param::Text(tally.tally.to_string()),
        Param::Text(tally.week.to_string()),
        Param::Text(tally.version.to_string()),
        Param::Text(tally.channel.as_str().to_owned()),
        Param::Text(tally.os.as_str().to_owned()),
        os_major(tally.os_major),
        Param::Text(tally.arch.as_str().to_owned()),
        Param::Text(tally.install_type.as_str().to_owned()),
        Param::Int(i32::from(tally.managed)),
        Param::Text(canonical(&tally.cells)?),
    ])
}

fn os_major(major: Option<OsMajor>) -> Param {
    major.map_or(Param::Null, |major| Param::Int(i32::from(major.get())))
}

/// The cells as the crate writes them: no whitespace, fields in the crate's order.
fn canonical<T: serde::Serialize>(cells: &T) -> Result<String, Refusal> {
    Ok(serde_json::to_string(cells)?)
}

#[cfg(test)]
mod tests;
