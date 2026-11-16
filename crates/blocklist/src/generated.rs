//! When a list was made (`generated`, from its trusted comment), and the rollback rule.

use std::fmt;

/// A list's `generated` time: RFC 3339 UTC to the second, `YYYY-MM-DDTHH:MM:SSZ`, checked to be
/// a real date and time. Fixed width, so the byte order is the time order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generated([u8; 20]);

impl Generated {
    /// Reads `YYYY-MM-DDTHH:MM:SSZ` exactly: no fraction, no offset other than `Z`, no lowercase
    /// `t` or `z`, and a day that exists in its month.
    pub fn parse(text: &str) -> Option<Generated> {
        let bytes: [u8; 20] = text.as_bytes().try_into().ok()?;
        let shape = b"dddd-dd-ddTdd:dd:ddZ";
        let shaped = bytes.iter().zip(shape).all(|(byte, want)| match want {
            b'd' => byte.is_ascii_digit(),
            _ => byte == want,
        });
        if !shaped {
            return None;
        }
        let generated = Generated(bytes);
        let (year, month, day) = (
            generated.field(0, 4),
            generated.field(5, 2),
            generated.field(8, 2),
        );
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            _ => return None,
        };
        let time_ok = generated.field(11, 2) < 24
            && generated.field(14, 2) < 60
            && generated.field(17, 2) < 60;
        ((1..=days).contains(&day) && time_ok).then_some(generated)
    }

    /// The decimal number in `len` digits from `at`; the shape check made them digits.
    fn field(&self, at: usize, len: usize) -> i64 {
        self.0
            .iter()
            .skip(at)
            .take(len)
            .fold(0, |value, digit| value * 10 + i64::from(digit - b'0'))
    }

    /// The text, `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn as_str(&self) -> &str {
        // Every byte passed the ASCII shape check.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// The date part, `YYYY-MM-DD`, for "list of <date>".
    pub fn date(&self) -> &str {
        self.as_str().get(..10).unwrap_or_default()
    }

    /// Seconds since the Unix epoch.
    pub fn unix_seconds(&self) -> i64 {
        let (year, month, day) = (self.field(0, 4), self.field(5, 2), self.field(8, 2));
        // Howard Hinnant's days_from_civil.
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let of_era = year - era * 400;
        let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
        let days = era * 146_097 + of_era_days - 719_468;
        days * 86_400 + self.field(11, 2) * 3_600 + self.field(14, 2) * 60 + self.field(17, 2)
    }
}

impl serde::Serialize for Generated {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl fmt::Display for Generated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The rollback rule: may a list made at `candidate` replace the one in use? Only when it is
/// newer than the list in use (`None`: no list loaded) and not older than the bundled snapshot
/// (`None`: no snapshot). An equal time is the same list: no change.
pub fn accept(candidate: Generated, in_use: Option<Generated>, bundled: Option<Generated>) -> bool {
    in_use.is_none_or(|in_use| candidate > in_use)
        && bundled.is_none_or(|bundled| candidate >= bundled)
}
