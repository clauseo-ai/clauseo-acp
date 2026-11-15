//! `POST /v1/crash` (decision 0062): one crash report, read through the crate's own
//! [`parse_crash`], kept as its re-serialised JSON in R2 at `reports/<report id>.json` and as
//! one `crash` row of its closed fields in D1. At most 200 reports are kept an hour, counted in
//! `crash_rate`, which holds an hour and a count and nothing else. Everything here is pure: the
//! Worker's glue sends the statements and the object.

use clauseo_counts::schema::{CrashKind, CrashReport, Time, crash_body, parse_crash};

use crate::route::Refusal;
use crate::rows::Param;

/// The most reports kept in one UTC hour, across every sender.
pub(crate) const HOURLY_CAP: i32 = 200;

/// The R2 binding in `wrangler.toml`. Without it (the bucket does not exist yet) the route
/// answers `503`.
pub(crate) const BUCKET: &str = "CRASH";

/// A report's row; the report id is the key, so a report sent again changes nothing.
pub(crate) const INSERT_CRASH: &str = "INSERT OR IGNORE INTO crash \
     (report, week, version, os, arch, kind, area, signature) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

/// Adds one to the hour's count (`?1`) unless it has reached the cap (`?2`), and returns the
/// new count; at the cap no row comes back and nothing changes.
pub(crate) const BUMP_CRASH_RATE: &str = "INSERT INTO crash_rate (hour, n) VALUES (?1, 1) \
     ON CONFLICT (hour) DO UPDATE SET n = n + 1 WHERE n < ?2 RETURNING n";

/// Deletes the counts of hours before this one, when the first report of an hour is counted.
pub(crate) const DELETE_PAST_HOURS: &str = "DELETE FROM crash_rate WHERE hour < ?1";

/// A report that passed the schema: its object and its row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upload {
    /// `reports/<report id>.json`.
    pub(crate) key: String,
    /// The report as the crate writes it: no whitespace, fields in the crate's order.
    pub(crate) body: Vec<u8>,
    /// The values of [`INSERT_CRASH`].
    pub(crate) row: Vec<Param>,
}

/// Reads an inflated body through the crate's parser, with this week taken from `now`, and
/// makes the object and the row. Anything the parser refuses is [`Refusal::Bad`].
pub(crate) fn accept(json: &[u8], now: Time) -> Result<Upload, Refusal> {
    let this_week = now.day().ok_or(Refusal::Failed)?.week();
    let report = parse_crash(json, this_week)?;
    let body = crash_body(&report, this_week)?;
    Ok(Upload {
        key: format!("reports/{}.json", report.report),
        body,
        row: vec![
            Param::Text(report.report.to_string()),
            Param::Text(report.week.to_string()),
            Param::Text(report.version.to_string()),
            Param::Text(report.os.as_str().to_owned()),
            Param::Text(report.arch.as_str().to_owned()),
            Param::Text(report.kind.as_str().to_owned()),
            Param::Text(report.area.as_str().to_owned()),
            Param::Text(signature(&report)),
        ],
    })
}

/// What groups like crashes together, from the closed fields only: a panic's `file:line` (or
/// `panic` when the file fit neither form), a signal's `code:<number>`, a renderer's reason.
pub(crate) fn signature(report: &CrashReport) -> String {
    match report.kind {
        CrashKind::Panic => report.location.as_ref().map_or_else(
            || "panic".to_owned(),
            |at| format!("{}:{}", at.file, at.line.get()),
        ),
        CrashKind::Signal => report.code.map_or_else(
            || "signal".to_owned(),
            |code| format!("code:{}", code.get()),
        ),
        CrashKind::RendererGone => report
            .reason
            .map_or("renderer_gone", |reason| reason.as_str())
            .to_owned(),
        CrashKind::Stall => "stall".to_owned(),
    }
}

/// The UTC hour of `now`, as `YYYY-MM-DDTHH`, which sorts as it reads.
pub(crate) fn hour(now: Time) -> Option<String> {
    let day = now.day()?;
    let hour = (now.unix_secs() % 86_400) / 3_600;
    Some(format!("{day}T{hour:02}"))
}

#[cfg(test)]
mod tests;
