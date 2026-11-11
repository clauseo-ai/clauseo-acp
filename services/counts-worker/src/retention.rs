//! The weekly cron's deletes (Mondays at 03:00 UTC): day reports after 90 days, tallies after
//! 396, and the hourly counts after 24 hours. Three `DELETE`s with their cut-offs worked out from
//! the Worker's clock through the crate's own [`Day`] and [`Week`], so the Worker reads no row.

use clauseo_counts::schema::{Day, Time, Week};

use crate::rows::Param;

/// How many days a `report` row is kept.
pub(crate) const REPORT_DAYS: i64 = 90;
/// How many days a `tally` row is kept, counted to the Monday of its week.
pub(crate) const TALLY_DAYS: i64 = 396;
/// How many hours a `rate` row is kept.
pub(crate) const RATE_HOURS: i32 = 24;

/// Deletes every report whose day is before the cut-off day.
const DELETE_REPORTS: &str = "DELETE FROM report WHERE day < ?1";
/// Deletes every tally whose week is before the cut-off week. `YYYY-Www` sorts as it reads.
const DELETE_TALLIES: &str = "DELETE FROM tally WHERE week < ?1";
/// Deletes every hourly count before the cut-off hour.
const DELETE_RATES: &str = "DELETE FROM rate WHERE hour < ?1";

/// The UTC hour `now` falls in, as hours since 1970: the `rate` table's key.
pub(crate) fn hour(now: Time) -> Option<i32> {
    i32::try_from(now.unix_secs() / 3_600).ok()
}

/// The three deletes for a run at `now`, or `None` when the clock is outside the years a
/// [`Day`] holds.
pub(crate) fn deletes(now: Time) -> Option<[(&'static str, Param); 3]> {
    let today = now.day()?;
    let reports: Day = today.days_before(REPORT_DAYS)?;
    let tallies: Week = today.days_before(TALLY_DAYS)?.week();
    let rates = hour(now)?.checked_sub(RATE_HOURS)?;
    Some([
        (DELETE_REPORTS, Param::Text(reports.to_string())),
        (DELETE_TALLIES, Param::Text(tallies.to_string())),
        (DELETE_RATES, Param::Int(rates)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cut_offs_are_90_days_396_days_and_24_hours_back() {
        // Monday 2026-11-09, 03:00 UTC.
        let monday = Day::from_ymd(2026, 11, 9).unwrap();
        let now = monday.start().plus(3 * 3_600);
        let hour_now = hour(now).unwrap();
        assert_eq!(i64::from(hour_now), monday.number() * 24 + 3);
        let [reports, tallies, rates] = deletes(now).unwrap();
        assert_eq!(
            reports,
            (DELETE_REPORTS, Param::Text("2026-08-11".to_owned()))
        );
        // 2025-10-09 is a Thursday in ISO week 41 of 2025.
        assert_eq!(
            tallies,
            (DELETE_TALLIES, Param::Text("2025-W41".to_owned()))
        );
        assert_eq!(rates, (DELETE_RATES, Param::Int(hour_now - 24)));
    }

    #[test]
    fn a_kept_day_is_at_most_90_days_old() {
        let today = Day::from_ymd(2026, 11, 9).unwrap();
        let [(_, Param::Text(cut)), ..] = deletes(today.start()).unwrap() else {
            unreachable!()
        };
        let cut: Day = cut.parse().unwrap();
        assert_eq!(today.days_since(cut), 90);
    }
}
