//! Time as data: [`Time`] (UTC seconds, which the caller reads from its clock), the UTC [`Day`]
//! and the ISO [`Week`]. A day travels as `2026-11-02`, a week as `2026-W45`; nothing finer.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::text::as_text;

/// Seconds in a day.
pub const DAY_SECS: u64 = 86_400;

/// The first and last years a [`Day`] may hold, so a date is always four digits and no clock
/// error far from now can travel.
const YEARS: std::ops::RangeInclusive<i64> = 2000..=2999;

/// A moment, as whole seconds since 1970-01-01 UTC. The crate never reads a clock: the caller
/// passes `Time` in. It never travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Time(u64);

impl Time {
    /// The moment `secs` seconds after 1970-01-01 UTC.
    pub const fn from_unix_secs(secs: u64) -> Time {
        Time(secs)
    }

    /// Seconds since 1970-01-01 UTC.
    pub const fn unix_secs(self) -> u64 {
        self.0
    }

    /// `secs` seconds later, stopping at the end of time.
    #[must_use]
    pub const fn plus(self, secs: u64) -> Time {
        Time(self.0.saturating_add(secs))
    }

    /// Seconds from `earlier` to `self`, or 0 when `earlier` is later.
    pub const fn since(self, earlier: Time) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The UTC day this moment falls in, or `None` outside the years a [`Day`] holds.
    pub fn day(self) -> Option<Day> {
        Day::from_number(i64::try_from(self.0 / DAY_SECS).ok()?)
    }
}

/// A UTC date from 2000 to 2999, kept as days since 1970-01-01. Serialised as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(i64);

impl Day {
    /// The day with this number (days since 1970-01-01), or `None` outside 2000 to 2999.
    pub fn from_number(number: i64) -> Option<Day> {
        let (year, _, _) = civil_from_days(number);
        YEARS.contains(&year).then_some(Day(number))
    }

    /// The date, or `None` when it does not exist or falls outside 2000 to 2999.
    pub fn from_ymd(year: i64, month: i64, day: i64) -> Option<Day> {
        let valid = YEARS.contains(&year)
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day);
        valid.then(|| Day(days_from_civil(year, month, day)))
    }

    /// Days since 1970-01-01.
    pub const fn number(self) -> i64 {
        self.0
    }

    /// The moment the day starts, 00:00 UTC.
    pub fn start(self) -> Time {
        Time(u64::try_from(self.0).unwrap_or(0).saturating_mul(DAY_SECS))
    }

    /// The day `days` days earlier (later for a negative `days`), or `None` outside 2000 to 2999.
    pub fn days_before(self, days: i64) -> Option<Day> {
        Day::from_number(self.0.checked_sub(days)?)
    }

    /// Whole days from `earlier` to `self` (negative when `earlier` is later).
    pub const fn days_since(self, earlier: Day) -> i64 {
        self.0 - earlier.0
    }

    /// The ISO week the day falls in.
    pub fn week(self) -> Week {
        Week(self.0 - weekday(self.0))
    }
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_days(self.0);
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// A string that is not a date or week in the one form the schema allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadDate;

impl fmt::Display for BadDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a date in the schema's form")
    }
}

impl std::error::Error for BadDate {}

impl FromStr for Day {
    type Err = BadDate;

    /// Exactly `YYYY-MM-DD`, a real date from 2000 to 2999.
    fn from_str(text: &str) -> Result<Day, BadDate> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(BadDate);
        }
        let year = digits(&bytes[0..4])?;
        let month = digits(&bytes[5..7])?;
        let day = digits(&bytes[8..10])?;
        Day::from_ymd(year, month, day).ok_or(BadDate)
    }
}

/// An ISO 8601 week, kept as the number of its Monday. Serialised as `YYYY-Www`, the ISO year
/// (which can differ from the calendar year near January).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Week(i64);

impl Week {
    /// The week's Monday.
    pub const fn monday(self) -> Day {
        Day(self.0)
    }

    /// The week after.
    #[must_use]
    pub const fn next(self) -> Week {
        Week(self.0 + 7)
    }

    /// The week `weeks` weeks earlier (later for a negative `weeks`), or `None` when its
    /// Monday falls outside 2000 to 2999.
    pub fn weeks_before(self, weeks: i64) -> Option<Week> {
        let monday = Day::from_number(self.0.checked_sub(weeks.checked_mul(7)?)?)?;
        Some(Week(monday.0))
    }

    /// Whole weeks from `earlier` to `self` (negative when `earlier` is later).
    pub const fn weeks_since(self, earlier: Week) -> i64 {
        (self.0 - earlier.0) / 7
    }

    /// The ISO year and week number.
    fn year_and_number(self) -> (i64, i64) {
        let thursday = self.0 + 3;
        let (year, _, _) = civil_from_days(thursday);
        let number = (thursday - days_from_civil(year, 1, 1)) / 7 + 1;
        (year, number)
    }

    /// Week `number` of ISO year `year`, or `None` when the year has no such week.
    fn from_year_and_number(year: i64, number: i64) -> Option<Week> {
        if !YEARS.contains(&year) || number < 1 {
            return None;
        }
        let jan4 = days_from_civil(year, 1, 4);
        let week = Week(jan4 - weekday(jan4) + (number - 1) * 7);
        (week.year_and_number() == (year, number)).then_some(week)
    }
}

impl fmt::Display for Week {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, number) = self.year_and_number();
        write!(f, "{year:04}-W{number:02}")
    }
}

impl FromStr for Week {
    type Err = BadDate;

    /// Exactly `YYYY-Www`, a week the ISO year has.
    fn from_str(text: &str) -> Result<Week, BadDate> {
        let bytes = text.as_bytes();
        if bytes.len() != 8 || bytes[4] != b'-' || bytes[5] != b'W' {
            return Err(BadDate);
        }
        let year = digits(&bytes[0..4])?;
        let number = digits(&bytes[6..8])?;
        Week::from_year_and_number(year, number).ok_or(BadDate)
    }
}

/// The pattern of a day on the wire; the schema test allows strings of this form.
pub const DAY_PATTERN: &str = "^2[0-9]{3}-[0-9]{2}-[0-9]{2}$";
/// The pattern of a week on the wire.
pub const WEEK_PATTERN: &str = "^2[0-9]{3}-W[0-9]{2}$";

as_text!(Day, "Day", DAY_PATTERN);
as_text!(Week, "Week", WEEK_PATTERN);

/// A day from 2000-01-01 (day 10,957) through 2999.
#[cfg(test)]
impl super::tests::sample::Sample for Day {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        let offset = i64::try_from(tape.below(365_000)).unwrap();
        Day::from_number(10_957 + offset).unwrap()
    }
}

#[cfg(test)]
impl super::tests::sample::Sample for Week {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        // From 2000-01-03, the first Monday of 2000, so the week's ISO year is 2000 or later.
        let offset = i64::try_from(tape.below(364_000)).unwrap();
        Day::from_number(10_959 + offset).unwrap().week()
    }
}

/// ASCII digits as a number; anything else is [`BadDate`].
fn digits(bytes: &[u8]) -> Result<i64, BadDate> {
    bytes.iter().try_fold(0_i64, |n, &b| {
        b.is_ascii_digit()
            .then(|| n * 10 + i64::from(b - b'0'))
            .ok_or(BadDate)
    })
}

/// Monday 0 to Sunday 6, for a day number (1970-01-01 was a Thursday).
const fn weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

const fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

const fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date of a day number (Howard Hinnant's `civil_from_days`).
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_print_and_parse_both_ways() {
        let day = Day::from_ymd(2026, 11, 2).unwrap();
        assert_eq!(day.to_string(), "2026-11-02");
        assert_eq!("2026-11-02".parse::<Day>(), Ok(day));
        assert_eq!(day.start().day(), Some(day));
        assert_eq!(
            Day::from_ymd(2024, 2, 29).unwrap().to_string(),
            "2024-02-29"
        );
    }

    #[test]
    fn a_day_in_any_other_form_is_refused() {
        for text in [
            "2026-11-2",
            "2026-13-01",
            "2025-02-29",
            "1999-12-31",
            "3000-01-01",
            "2026/11/02",
            " 2026-11-02",
            "2026-11-02T00:00:00Z",
            "+026-11-02",
            "",
        ] {
            assert_eq!(text.parse::<Day>(), Err(BadDate), "{text}");
        }
    }

    #[test]
    fn iso_weeks_follow_the_thursday_rule() {
        let cases = [
            ((2026, 11, 2), "2026-W45"),
            ((2026, 1, 1), "2026-W01"),
            ((2021, 1, 3), "2020-W53"),
            ((2024, 12, 30), "2025-W01"),
            ((2027, 1, 3), "2026-W53"),
        ];
        for ((y, m, d), want) in cases {
            let week = Day::from_ymd(y, m, d).unwrap().week();
            assert_eq!(week.to_string(), want);
            assert_eq!(want.parse::<Week>(), Ok(week));
            assert_eq!(weekday(week.monday().number()), 0);
        }
        assert_eq!("2025-W53".parse::<Week>(), Err(BadDate));
        assert_eq!("2026-W00".parse::<Week>(), Err(BadDate));
        assert_eq!("2026-W5".parse::<Week>(), Err(BadDate));
    }

    #[test]
    fn a_day_number_round_trips_through_its_date() {
        for number in (10_957..1_000_000).step_by(997) {
            let Some(day) = Day::from_number(number) else {
                continue;
            };
            assert_eq!(day.to_string().parse::<Day>(), Ok(day));
        }
    }
}
