//! The pool of sandbox users (design B, D9): one local user per matter, each named
//! `ClauseoSbNN` ([`pool_name`]), all in the group `ClauseoSandboxUsers`, at most [`POOL_CAP`] on
//! a machine (D-B5). The machine's record of them ([`MachineRecord`]) is
//! `HKLM\SOFTWARE\Clauseo\Sandbox`, which the elevated set-up writes (S15 `PR4e`) and `record.rs`
//! reads, and [`pool_sids_from_multi_sz`] decodes its `PoolSids` value. Which matter gets which
//! pool user, and when one may go to another matter, is [`assign`].
//!
//! Pure: built on every OS so its tests run everywhere.

use std::collections::BTreeSet;
use std::path::PathBuf;

use super::sids::Sid;

pub mod assign;

/// The most pool users a machine has (D-B5: 4 at first, 4 more at a time, at most 32).
pub const POOL_CAP: u8 = 32;
/// The start of every pool user's name; two decimal digits follow.
pub const POOL_PREFIX: &str = "ClauseoSb";
/// The local group every pool user is in.
pub const POOL_GROUP: &str = "ClauseoSandboxUsers";

/// One pool user: its account name and its SID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolUser {
    /// `ClauseoSbNN` ([`pool_name`]).
    pub name: String,
    /// The account's SID, as `LookupAccountNameW` gives it.
    pub sid: Sid,
}

/// The name of pool user `n`: `ClauseoSb01` to `ClauseoSb32`. `None` when `n` is 0 or above
/// [`POOL_CAP`].
pub fn pool_name(n: u8) -> Option<String> {
    (1..=POOL_CAP)
        .contains(&n)
        .then(|| format!("{POOL_PREFIX}{n:02}"))
}

/// The number in a pool user's name, the inverse of [`pool_name`]: `Some(n)` only for exactly
/// the name `pool_name(n)` gives (case and all), else `None`.
pub fn pool_number(name: &str) -> Option<u8> {
    let digits = name.strip_prefix(POOL_PREFIX)?;
    if digits.len() != 2 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let n: u8 = digits.parse().ok()?;
    pool_name(n).is_some().then_some(n)
}

/// What `HKLM\SOFTWARE\Clauseo\Sandbox` holds (D-B1). The set-up writes the key with the
/// protected DACL `D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)` (S15 `PR4e`): SYSTEM and
/// Administrators full, Users read, since #317 reads it under a pool token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineRecord {
    /// `OwnerSid` (`REG_SZ`): the Windows user who set the pool up (D5).
    pub owner: Sid,
    /// `GroupSid` (`REG_SZ`): the `ClauseoSandboxUsers` group's SID.
    pub group: Sid,
    /// `PoolSids` (`REG_MULTI_SZ`): every pool user's SID.
    pub pool: Vec<Sid>,
    /// `DataDir` (`REG_SZ`): the owner's data folder, which holds `sandbox\`.
    pub data_dir: PathBuf,
    /// `Removing` (`REG_DWORD`, non-zero): a removal started and has not finished.
    pub removing: bool,
}

/// Why a `REG_MULTI_SZ` is not a list of SIDs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MultiSzError {
    /// An empty string before the end: two NULs inside the list.
    #[error("the list holds an empty string")]
    EmptyString,
    /// A string that is not UTF-16.
    #[error("the list holds a string that is not UTF-16")]
    NotText,
    /// A string that is not a SID in `Sid`'s form.
    #[error("the list holds {0}")]
    NotASid(String),
    /// The same SID twice.
    #[error("the list holds {0} twice")]
    Repeated(String),
}

/// The SIDs in a `REG_MULTI_SZ`'s UTF-16 units: strings each ended by a NUL, the list ended by
/// one more. Trailing NULs are dropped, so an empty value, `[0]` and `[0, 0]` are an empty list
/// and a last string without its NULs still reads. Anything else fails closed: an empty string
/// before the end, a string that is not UTF-16 or not a SID, or a SID listed twice.
pub fn pool_sids_from_multi_sz(units: &[u16]) -> Result<Vec<Sid>, MultiSzError> {
    let end = units
        .iter()
        .rposition(|unit| *unit != 0)
        .map_or(0, |last| last + 1);
    let body = units.get(..end).unwrap_or_default();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = BTreeSet::new();
    let mut sids = Vec::new();
    for part in body.split(|unit| *unit == 0) {
        if part.is_empty() {
            return Err(MultiSzError::EmptyString);
        }
        let text = String::from_utf16(part).map_err(|_not_utf16| MultiSzError::NotText)?;
        let sid = Sid::parse(&text).map_err(|error| MultiSzError::NotASid(error.to_string()))?;
        if !seen.insert(sid.clone()) {
            return Err(MultiSzError::Repeated(sid.to_string()));
        }
        sids.push(sid);
    }
    Ok(sids)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `strings` as a `REG_MULTI_SZ` writes them: each ended by a NUL, then one more.
    fn multi_sz(strings: &[&str]) -> Vec<u16> {
        let mut units: Vec<u16> = Vec::new();
        for text in strings {
            units.extend(text.encode_utf16());
            units.push(0);
        }
        units.push(0);
        units
    }

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    const ONE: &str = "S-1-5-21-7-8-9-1010";
    const TWO: &str = "S-1-5-21-7-8-9-1011";

    #[test]
    fn pool_names_run_from_01_to_32() {
        assert_eq!(pool_name(1).as_deref(), Some("ClauseoSb01"));
        assert_eq!(pool_name(9).as_deref(), Some("ClauseoSb09"));
        assert_eq!(pool_name(32).as_deref(), Some("ClauseoSb32"));
        assert_eq!(pool_name(0), None);
        assert_eq!(pool_name(33), None);
        assert_eq!(pool_name(u8::MAX), None);
        for n in 1..=POOL_CAP {
            let name = pool_name(n).unwrap();
            // A local account name is at most 20 characters.
            assert!(name.len() <= 20, "{name}");
            assert_eq!(pool_number(&name), Some(n), "{name}");
        }
    }

    #[test]
    fn only_the_exact_pool_names_have_a_number() {
        for other in [
            "ClauseoSb00",
            "ClauseoSb33",
            "ClauseoSb1",
            "ClauseoSb001",
            "ClauseoSb+1",
            "clauseosb01",
            "ClauseoSandbox",
            "ClauseoSb01 ",
            "ClauseoSb١٢",
            "",
        ] {
            assert_eq!(pool_number(other), None, "{other:?}");
        }
    }

    #[test]
    fn an_empty_multi_sz_is_an_empty_pool() {
        for empty in [&[][..], &[0], &[0, 0], &[0, 0, 0]] {
            assert_eq!(pool_sids_from_multi_sz(empty), Ok(vec![]), "{empty:?}");
        }
        assert_eq!(pool_sids_from_multi_sz(&multi_sz(&[])), Ok(vec![]));
    }

    #[test]
    fn a_multi_sz_reads_each_sid_in_order() {
        let both = multi_sz(&[ONE, TWO]);
        assert!(both.ends_with(&[0, 0]));
        assert_eq!(pool_sids_from_multi_sz(&both), Ok(vec![sid(ONE), sid(TWO)]));
        // Without its final NULs, or with extra ones, the same list.
        let bare: Vec<u16> = both.iter().copied().take(both.len() - 2).collect();
        assert_eq!(pool_sids_from_multi_sz(&bare), Ok(vec![sid(ONE), sid(TWO)]));
        let padded: Vec<u16> = both.iter().copied().chain([0, 0]).collect();
        assert_eq!(
            pool_sids_from_multi_sz(&padded),
            Ok(vec![sid(ONE), sid(TWO)])
        );
    }

    #[test]
    fn a_malformed_multi_sz_fails_closed() {
        assert_eq!(
            pool_sids_from_multi_sz(&multi_sz(&[ONE, "", TWO])),
            Err(MultiSzError::EmptyString)
        );
        assert_eq!(
            pool_sids_from_multi_sz(&multi_sz(&["", ONE])),
            Err(MultiSzError::EmptyString)
        );
        for bad in ["BA", "not a sid", "S-1-5-021", " S-1-5-18"] {
            assert!(
                matches!(
                    pool_sids_from_multi_sz(&multi_sz(&[ONE, bad])),
                    Err(MultiSzError::NotASid(_))
                ),
                "{bad:?}"
            );
        }
        // A lone surrogate is not UTF-16.
        let mut broken = multi_sz(&[ONE]);
        broken.splice(0..0, [0xD800, 0]);
        assert_eq!(pool_sids_from_multi_sz(&broken), Err(MultiSzError::NotText));
        assert_eq!(
            pool_sids_from_multi_sz(&multi_sz(&[ONE, TWO, ONE])),
            Err(MultiSzError::Repeated(ONE.to_owned()))
        );
    }
}
