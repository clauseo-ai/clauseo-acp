//! The native crash record: `<data_dir>/crash/native`, opened empty at start with a header
//! naming this run's version, to which a native crash handler appends one line of fixed form
//! ([`native_line`]) with async-signal-safe calls. The next start reads it back ([`report`]).
//! The handler sees only real faults, so a user's kill, a power loss or a shutdown leaves the
//! record as the header alone, and no report.

use crate::about::About;
use crate::schema::{CrashReport, SignalCode, Time, Version};

use super::record;

/// The longest line [`native_line`] makes.
pub const NATIVE_LINE_MAX: usize = 32;

/// The header's first word, and the line's.
const HEADER: &str = "clauseo-native 1 ";
const LINE: &str = "signal ";

/// The header written at start: the record's form and this run's version, so a report made at
/// the next start (perhaps after an update) names the version that crashed.
pub(crate) fn header(about: &About) -> Vec<u8> {
    format!("{HEADER}{}\n", about.version).into_bytes()
}

/// The handler's line for `code`, in `buf`: `signal <code> <0|1>\n`, the last field
/// `after_launch`. No allocation, no lock and no call: safe in a signal handler.
pub fn native_line(code: u32, after_launch: bool, buf: &mut [u8; NATIVE_LINE_MAX]) -> &[u8] {
    let mut len = 0;
    for &b in LINE.as_bytes() {
        buf[len] = b;
        len += 1;
    }
    let mut digits = [0_u8; 10];
    let mut count = 0;
    let mut rest = code;
    loop {
        digits[count] = b'0' + u8::try_from(rest % 10).unwrap_or(0);
        count += 1;
        rest /= 10;
        if rest == 0 || count == digits.len() {
            break;
        }
    }
    while count > 0 {
        count -= 1;
        buf[len] = digits[count];
        len += 1;
    }
    buf[len] = b' ';
    buf[len + 1] = if after_launch { b'1' } else { b'0' };
    buf[len + 2] = b'\n';
    &buf[..len + 3]
}

/// The report a record holds, if its handler wrote a line: the header's version with the rest
/// of `about`, the line's code and `after_launch`, found at `now`. A record of the header alone,
/// or in any other form, holds none.
pub(crate) fn report(bytes: &[u8], about: &About, now: Time) -> Option<CrashReport> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    let version: Version = lines.next()?.strip_prefix(HEADER)?.parse().ok()?;
    let mut fields = lines.next()?.strip_prefix(LINE)?.split(' ');
    let code = SignalCode::new(fields.next()?.parse().ok()?)?;
    let after_launch = match fields.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let about = About { version, ..*about };
    record::signal(&about, after_launch, code, now)
}
