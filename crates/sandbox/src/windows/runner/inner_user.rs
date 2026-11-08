//! The outer runner's check of the suspended inner runner's user (M1), apart from its calls so
//! the tests hold it on every OS.
//!
//! The first try is the inner's token (`OpenProcessToken(TOKEN_QUERY)`), which works on
//! `WinSta0` or elevated. On the agent's own station the Secondary Logon service gives the pool
//! logon a fresh logon SID, and the token's DACL (pool user, `SYSTEM`, that logon SID,
//! `Administrators` for `TOKEN_SWITCH` only) no longer names the desktop user, so an unelevated
//! outer is refused (5). Then the user is read without the token:
//! `NtQueryInformationProcess(ProcessTelemetryIdInformation)` through the process handle the
//! logon gave ([`telemetry_user`]), which needs only `PROCESS_QUERY_LIMITED_INFORMATION`; the
//! kernel copies the token's user into the buffer with no check of the token's DACL. It is
//! undocumented, so any doubt in the buffer fails the check, and the buffer must name the
//! inner's pid. The outer holds the inner's process handle, so the pid can't be reused while it
//! is suspended.
//!
//! `WTSEnumerateProcessesExW` is no way: on Windows Server 2025 (26100) a Limited desktop user's
//! list has the suspended inner's pid with a NULL `pUserSid`, where the telemetry call gave
//! `STATUS_SUCCESS`, 456 bytes, `UserSidOffset` 96 and the pool user's SID.
//!
//! Only the user is checked, as the token check did. A mismatch, or both ways failing, is
//! `RunExit::Logon` with the way named.

use std::fmt::Write as _;
use std::io;

use super::RunExit;
use crate::windows::sids::Sid;

#[cfg(windows)]
pub(super) mod os;

/// `ERROR_ACCESS_DENIED`.
const ACCESS_DENIED: i32 = 5;

/// Checks the inner's user is `pool_user`: from its token (`token`), or, when that was refused
/// (5), from the telemetry buffer (`telemetry`). Any failure is [`RunExit::Logon`], its message
/// naming the way that failed.
pub(super) fn check_inner_user(
    pool_user: &Sid,
    token: io::Result<Sid>,
    telemetry: impl FnOnce() -> Result<Sid, String>,
) -> Result<(), (RunExit, String)> {
    let refused = match token {
        Ok(user) => return compare(pool_user, &user, "its token"),
        Err(error) if error.raw_os_error() == Some(ACCESS_DENIED) => error,
        Err(error) => return Err((RunExit::Logon, token_failed(&error))),
    };
    match telemetry() {
        Ok(user) => compare(pool_user, &user, "ProcessTelemetryIdInformation"),
        Err(error) => Err((
            RunExit::Logon,
            format!(
                "the inner runner's user can't be read: {}; {error}",
                token_failed(&refused)
            ),
        )),
    }
}

fn token_failed(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => format!("the inner runner's token failed ({code})"),
        None => format!("the inner runner's token failed ({})", error.kind()),
    }
}

fn compare(pool_user: &Sid, user: &Sid, how: &str) -> Result<(), (RunExit, String)> {
    if user == pool_user {
        Ok(())
    } else {
        Err((
            RunExit::Logon,
            format!("the inner runner isn't the pool user (by {how})"),
        ))
    }
}

// `PROCESS_TELEMETRY_ID_INFORMATION`'s fields read here, as phnt declares the struct
// (`phnt/include/ntpsapi.h`, also System Informer's): `ULONG HeaderSize` at 0, `ULONG ProcessId`
// at 4, then six `ULONGLONG`s (8 to 55), `ULONG SessionId`, `BootId`, `ImageChecksum`,
// `ImageTimeDateStamp` (56 to 71), and `ULONG UserSidOffset` at 72, an offset from the buffer's
// start to the user's SID.
const HEADER_SIZE_AT: usize = 0;
const PROCESS_ID_AT: usize = 4;
const USER_SID_OFFSET_AT: usize = 72;
/// The fixed header must reach past `UserSidOffset`.
const HEADER_MIN: usize = USER_SID_OFFSET_AT + 4;

/// The user's SID in a `ProcessTelemetryIdInformation` buffer for process `pid`: the header
/// must hold `UserSidOffset` and name `pid`, and the SID must lie past the header and inside
/// the buffer, revision 1, with 1 to 15 sub-authorities. Gives the SID's offset too, for the
/// Windows half's `IsValidSid` on the same bytes.
pub(super) fn telemetry_sid(buffer: &[u8], pid: u32) -> Result<(usize, Sid), String> {
    let bad = |what: &str| format!("ProcessTelemetryIdInformation: {what}");
    let header = u32_at(buffer, HEADER_SIZE_AT).ok_or_else(|| bad("a short buffer"))?;
    let header = usize::try_from(header).map_err(|_e| bad("the header size"))?;
    if header < HEADER_MIN || header > buffer.len() {
        return Err(bad("the header size"));
    }
    if u32_at(buffer, PROCESS_ID_AT) != Some(pid) {
        return Err(bad("another process"));
    }
    let offset = u32_at(buffer, USER_SID_OFFSET_AT)
        .and_then(|o| usize::try_from(o).ok())
        .ok_or_else(|| bad("the user's offset"))?;
    if offset < header {
        return Err(bad("the user's offset"));
    }
    buffer
        .get(offset..)
        .and_then(sid_from_bytes)
        .map(|sid| (offset, sid))
        .ok_or_else(|| bad("the user's SID"))
}

/// [`telemetry_sid`]'s SID alone.
#[cfg(test)]
fn telemetry_user(buffer: &[u8], pid: u32) -> Result<Sid, String> {
    telemetry_sid(buffer, pid).map(|(_, sid)| sid)
}

/// The little-endian `u32` at `at`.
fn u32_at(buffer: &[u8], at: usize) -> Option<u32> {
    let bytes = buffer.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

/// The binary SID at the start of `bytes` (revision 1, a count of 1 to 15 sub-authorities, a
/// 6-byte big-endian authority, the little-endian sub-authorities), in [`Sid`]'s form; an
/// authority past 32 bits (the hex form [`Sid`] refuses) is `None`.
fn sid_from_bytes(bytes: &[u8]) -> Option<Sid> {
    let (&revision, rest) = bytes.split_first()?;
    let (&count, rest) = rest.split_first()?;
    if revision != 1 || !(1..=15).contains(&count) {
        return None;
    }
    let authority = rest.get(..6)?;
    let authority = authority
        .iter()
        .fold(0u64, |value, &byte| (value << 8) | u64::from(byte));
    let authority = u32::try_from(authority).ok()?;
    let mut text = format!("S-1-{authority}");
    for index in 0..usize::from(count) {
        let sub = u32_at(rest, 6 + 4 * index)?;
        write!(text, "-{sub}").ok()?;
    }
    Sid::parse(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POOL: &str = "S-1-5-21-1-2-3-1001";

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    fn denied() -> io::Result<Sid> {
        Err(io::Error::from_raw_os_error(5))
    }

    /// A binary SID: revision 1, authority 5, the sub-authorities.
    fn sid_bytes(subs: &[u32]) -> Vec<u8> {
        let mut bytes = vec![1, u8::try_from(subs.len()).unwrap(), 0, 0, 0, 0, 0, 5];
        for sub in subs {
            bytes.extend_from_slice(&sub.to_le_bytes());
        }
        bytes
    }

    /// A telemetry buffer for `pid`: a 96-byte header, the user's SID at offset 96, then an
    /// image path's bytes.
    fn buffer(pid: u32) -> Vec<u8> {
        let mut buffer = vec![0u8; 96];
        buffer[0..4].copy_from_slice(&96u32.to_le_bytes());
        buffer[4..8].copy_from_slice(&pid.to_le_bytes());
        buffer[72..76].copy_from_slice(&96u32.to_le_bytes());
        buffer.extend(sid_bytes(&[21, 1, 2, 3, 1001]));
        buffer.extend_from_slice(&[b'C', 0, b':', 0, 0, 0]);
        buffer
    }

    #[test]
    fn a_valid_telemetry_buffer_gives_the_user() {
        assert_eq!(telemetry_user(&buffer(41), 41), Ok(sid(POOL)));
    }

    #[test]
    fn a_doubtful_telemetry_buffer_fails() {
        let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
        cases.push(("another pid", buffer(42)));
        let full = buffer(41);
        cases.push(("truncated in the SID", full[..96 + 12].to_vec()));
        cases.push(("truncated in the header", full[..60].to_vec()));
        cases.push(("empty", Vec::new()));
        let mut past_end = buffer(41);
        past_end[72..76].copy_from_slice(&4096u32.to_le_bytes());
        cases.push(("offset past the end", past_end));
        let mut in_header = buffer(41);
        in_header[72..76].copy_from_slice(&8u32.to_le_bytes());
        cases.push(("offset inside the header", in_header));
        let mut zero = buffer(41);
        zero[72..76].copy_from_slice(&0u32.to_le_bytes());
        cases.push(("no offset", zero));
        let mut small_header = buffer(41);
        small_header[0..4].copy_from_slice(&72u32.to_le_bytes());
        cases.push(("a header short of the offset field", small_header));
        let mut big_header = buffer(41);
        big_header[0..4].copy_from_slice(&9999u32.to_le_bytes());
        cases.push(("a header past the buffer", big_header));
        let mut revision = buffer(41);
        revision[96] = 2;
        cases.push(("SID revision 2", revision));
        let mut no_subs = buffer(41);
        no_subs[97] = 0;
        cases.push(("no sub-authority", no_subs));
        let mut many = buffer(41);
        many[97] = 16;
        cases.push(("16 sub-authorities", many));
        let mut count_past = buffer(41);
        count_past[97] = 15;
        cases.push(("a count past the buffer", count_past));
        let mut wide = buffer(41);
        wide[98] = 1;
        cases.push(("an authority past 32 bits", wide));
        for (what, case) in cases {
            assert!(telemetry_user(&case, 41).is_err(), "{what}");
        }
    }

    #[test]
    fn the_token_decides_when_it_can_be_read() {
        let never_read = || -> Result<Sid, String> { panic!("telemetry") };
        assert_eq!(
            check_inner_user(&sid(POOL), Ok(sid(POOL)), never_read),
            Ok(())
        );
        assert_eq!(
            check_inner_user(&sid(POOL), Ok(sid("S-1-5-18")), never_read),
            Err((
                RunExit::Logon,
                "the inner runner isn't the pool user (by its token)".to_owned()
            ))
        );
        // Another failure than 5 is no reason to look elsewhere.
        assert_eq!(
            check_inner_user(&sid(POOL), Err(io::Error::from_raw_os_error(6)), never_read),
            Err((
                RunExit::Logon,
                "the inner runner's token failed (6)".to_owned()
            ))
        );
    }

    #[test]
    /// A user SID other than the pool user's, read from the telemetry after the token refused
    /// this process, is `RunExit::Logon`: the suspended inner is killed and never runs.
    fn a_refused_token_falls_back_to_the_telemetry() {
        assert_eq!(
            check_inner_user(&sid(POOL), denied(), || Ok(sid(POOL))),
            Ok(())
        );
        assert_eq!(
            check_inner_user(&sid(POOL), denied(), || Ok(sid("S-1-5-18"))),
            Err((
                RunExit::Logon,
                "the inner runner isn't the pool user (by ProcessTelemetryIdInformation)"
                    .to_owned()
            ))
        );
        // The real path: the buffer's parse, for the inner's pid only.
        assert_eq!(
            check_inner_user(&sid(POOL), denied(), || telemetry_user(&buffer(41), 41)),
            Ok(())
        );
        assert!(
            check_inner_user(&sid(POOL), denied(), || telemetry_user(&buffer(42), 41)).is_err()
        );
    }

    #[test]
    fn both_ways_failing_names_each() {
        assert_eq!(
            check_inner_user(&sid(POOL), denied(), || {
                Err("ProcessTelemetryIdInformation: the user's SID".to_owned())
            }),
            Err((
                RunExit::Logon,
                "the inner runner's user can't be read: the inner runner's token failed (5); \
                 ProcessTelemetryIdInformation: the user's SID"
                    .to_owned()
            ))
        );
    }
}
