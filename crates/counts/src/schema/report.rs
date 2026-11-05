//! The two payloads and their envelopes (research 37 §2.1): the day report, which carries the
//! install id, and the weekly tally, which never does. `POST /v1/counts` takes a [`Batch`] of 1
//! to 7 day reports; `POST /v1/tally` takes one [`TallyReport`]. The Worker reads both through
//! [`parse_batch`] and [`parse_tally`], the same rules the client keeps.

use super::closed::seal;
use super::{
    Arch, Channel, DayCells, InstallId, InstallType, Language, Os, OsMajor, Platform,
    SchemaVersion, TallyCells, TallyId, Version,
};
use super::{Day, Week};

/// The most day reports in one request.
pub const MAX_DAYS_PER_BATCH: usize = 7;
/// How old a day may be when it is sent: at most 14 days before today.
pub const MAX_DAY_AGE: i64 = 14;
/// How old a tally's week may be when it is sent: at most 4 weeks before this one.
pub const MAX_TALLY_AGE_WEEKS: i64 = 4;
/// The largest gzip body for `/v1/counts`, and the largest JSON it may inflate to.
pub const MAX_COUNTS_GZIP: usize = 16_000;
/// See [`MAX_COUNTS_GZIP`].
pub const MAX_COUNTS_JSON: usize = 64_000;
/// The largest gzip body for `/v1/tally`, and the largest JSON it may inflate to.
pub const MAX_TALLY_GZIP: usize = 4_000;
/// See [`MAX_TALLY_GZIP`].
pub const MAX_TALLY_JSON: usize = 16_000;

/// One install's counts for one finished UTC day.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct DayReport {
    /// `1`.
    pub schema: SchemaVersion,
    /// The install id.
    pub install: InstallId,
    /// The UTC day.
    pub day: Day,
    /// The app's version.
    pub version: Version,
    /// `stable` or `beta`.
    pub channel: Channel,
    /// The OS.
    pub os: Os,
    /// The OS's major version; Linux leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_major: Option<OsMajor>,
    /// The CPU architecture.
    pub arch: Arch,
    /// The app's language.
    pub language: Language,
    /// The day's non-zero cells.
    pub cells: DayCells,
}

seal!(DayReport {
    schema,
    install,
    day,
    version,
    channel,
    os,
    os_major,
    arch,
    language,
    cells
});

/// The body of `POST /v1/counts`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct Batch {
    /// 1 to 7 day reports, oldest first.
    pub days: Vec<DayReport>,
}

seal!(Batch { days });

/// One install's weekly tally: crashes and `active_week`, with the two fields that mark a
/// firm's fleet. No install id, so a crash can never be joined to one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
pub struct TallyReport {
    /// `1`.
    pub schema: SchemaVersion,
    /// This tally's own random id.
    pub tally: TallyId,
    /// The ISO week.
    pub week: Week,
    /// The app's version.
    pub version: Version,
    /// `stable` or `beta`.
    pub channel: Channel,
    /// The OS.
    pub os: Os,
    /// The OS's major version; Linux leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_major: Option<OsMajor>,
    /// The CPU architecture.
    pub arch: Arch,
    /// Installed for the user or for the machine.
    pub install_type: InstallType,
    /// Whether any policy source was read.
    pub managed: bool,
    /// The week's non-zero cells.
    pub cells: TallyCells,
}

seal!(TallyReport {
    schema,
    tally,
    week,
    version,
    channel,
    os,
    os_major,
    arch,
    install_type,
    managed,
    cells
});

/// Why a payload was refused. It carries no text, so a refusal can be answered with a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    /// Not JSON, or not the schema: a field or value it does not know, or a wrong type.
    Schema,
    /// Larger than its cap, compressed or as JSON.
    TooLarge,
    /// No day, or more than 7.
    DayCount,
    /// The same day twice in one batch.
    RepeatedDay,
    /// A day or week after today's, or older than the schema allows.
    OutOfWindow,
    /// An OS and major version the schema does not pair.
    Platform,
    /// Two cells with one combination, or more than one `time_open` or `active_week` cell.
    Cells,
    /// A report with no cell.
    Empty,
}

impl DayReport {
    /// The schema's rules beyond its types, for a report sent when `today` is the UTC day.
    pub fn check(&self, today: Day) -> Result<(), Invalid> {
        let age = today.days_since(self.day);
        if !(1..=MAX_DAY_AGE).contains(&age) {
            return Err(Invalid::OutOfWindow);
        }
        Platform::from_parts(self.os, self.os_major).ok_or(Invalid::Platform)?;
        if self.cells.is_malformed() {
            return Err(Invalid::Cells);
        }
        if self.cells.is_empty() {
            return Err(Invalid::Empty);
        }
        Ok(())
    }
}

impl Batch {
    /// Every rule of [`DayReport::check`], 1 to 7 days, and no day twice.
    pub fn check(&self, today: Day) -> Result<(), Invalid> {
        if !(1..=MAX_DAYS_PER_BATCH).contains(&self.days.len()) {
            return Err(Invalid::DayCount);
        }
        let mut seen = std::collections::BTreeSet::new();
        for report in &self.days {
            report.check(today)?;
            if !seen.insert(report.day) {
                return Err(Invalid::RepeatedDay);
            }
        }
        Ok(())
    }
}

impl TallyReport {
    /// The schema's rules beyond its types, for a tally sent in the week `this_week`: its own
    /// week has closed and is at most 4 weeks old.
    pub fn check(&self, this_week: Week) -> Result<(), Invalid> {
        let age = this_week.weeks_since(self.week);
        if !(1..=MAX_TALLY_AGE_WEEKS).contains(&age) {
            return Err(Invalid::OutOfWindow);
        }
        Platform::from_parts(self.os, self.os_major).ok_or(Invalid::Platform)?;
        if self.cells.is_malformed() {
            return Err(Invalid::Cells);
        }
        if self.cells.crash.is_empty() && self.cells.active_week.is_empty() {
            return Err(Invalid::Empty);
        }
        Ok(())
    }
}

/// Reads an inflated `/v1/counts` body: at most 64 KB of JSON, the strict types, then
/// [`Batch::check`].
pub fn parse_batch(json: &[u8], today: Day) -> Result<Batch, Invalid> {
    if json.len() > MAX_COUNTS_JSON {
        return Err(Invalid::TooLarge);
    }
    let batch: Batch = serde_json::from_slice(json).map_err(Invalid::from)?;
    batch.check(today)?;
    Ok(batch)
}

/// Reads an inflated `/v1/tally` body: at most 16 KB of JSON, the strict types, then
/// [`TallyReport::check`].
pub fn parse_tally(json: &[u8], this_week: Week) -> Result<TallyReport, Invalid> {
    if json.len() > MAX_TALLY_JSON {
        return Err(Invalid::TooLarge);
    }
    let tally: TallyReport = serde_json::from_slice(json).map_err(Invalid::from)?;
    tally.check(this_week)?;
    Ok(tally)
}

impl From<serde_json::Error> for Invalid {
    fn from(_: serde_json::Error) -> Invalid {
        Invalid::Schema
    }
}
