//! SIDs as text ([`Sid`]), the matter SID's shape (`S-1-5-21-<a>-<b>-<c>-<rid>`, random), and
//! the SIDs a matter SID must never be ([`Reserved`]): this machine's real accounts. The matter
//! SIDs' file is `matters.rs`.
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::collections::BTreeSet;
use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use crate::SetupError;

/// The fewest sub-authorities a SID may have here, and the most (`SID_MAX_SUB_AUTHORITIES`).
const SUB_AUTHORITIES: std::ops::RangeInclusive<usize> = 1..=15;
/// The largest identifier authority: 48 bits.
const AUTHORITY_MAX: u64 = (1 << 48) - 1;
/// A matter SID's relative id is at least this, clear of the well-known RIDs (500 and up).
const MATTER_RID_MIN: u32 = 1000;

/// A SID in its canonical string form: `S-1-<authority>-<sub>…`, decimal, no leading zeros, one
/// to fifteen sub-authorities, each a `u32`. Canonical, so two `Sid`s are equal exactly when they
/// name the same SID. `ConvertSidToStringSidW` writes this form; aliases such as `BA` and the
/// hex authority form are refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sid(String);

/// A string that is not a SID in [`Sid`]'s form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a SID: {0:?}")]
pub struct NotASid(pub String);

impl Sid {
    /// Checks `text` against [`Sid`]'s form.
    pub fn parse(text: &str) -> Result<Sid, NotASid> {
        let refuse = || NotASid(text.chars().take(200).collect());
        let rest = text.strip_prefix("S-1-").ok_or_else(refuse)?;
        let mut parts = rest.split('-');
        let authority = parts.next().and_then(decimal).ok_or_else(refuse)?;
        if authority > AUTHORITY_MAX {
            return Err(refuse());
        }
        let mut count = 0usize;
        for part in parts {
            let sub = decimal(part).ok_or_else(refuse)?;
            u32::try_from(sub).map_err(|_too_big| refuse())?;
            count += 1;
        }
        if !SUB_AUTHORITIES.contains(&count) {
            return Err(refuse());
        }
        Ok(Sid(text.to_owned()))
    }

    /// The SID's string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is a matter SID's shape: `S-1-5-21-<a>-<b>-<c>-<rid>`, `rid` at least 1000.
    pub fn is_matter_form(&self) -> bool {
        let subs: Vec<&str> = self.0.split('-').skip(3).collect();
        let rid = subs.last().and_then(|rid| rid.parse::<u32>().ok());
        self.0.starts_with("S-1-5-21-")
            && subs.len() == 5
            && rid.is_some_and(|rid| rid >= MATTER_RID_MIN)
    }

    /// The account domain of a SID under `S-1-5-21` (a machine's or a domain's): the three
    /// sub-authorities after `21`, when a relative id follows them or not. `None` for others.
    pub fn domain(&self) -> Option<[u32; 3]> {
        let rest = self.0.strip_prefix("S-1-5-21-")?;
        let mut parts = rest.split('-').map(|part| part.parse::<u32>().ok());
        Some([parts.next()??, parts.next()??, parts.next()??])
    }

    /// A new matter SID from the OS's randomness: three random sub-authorities after `21`, and a
    /// random relative id of at least 1000.
    pub(crate) fn random_matter() -> io::Result<Sid> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        let [a, b, c, r] = [0usize, 4, 8, 12].map(|at| {
            let mut word = [0u8; 4];
            word.copy_from_slice(bytes.get(at..at + 4).unwrap_or(&[0; 4]));
            u32::from_le_bytes(word)
        });
        let rid = MATTER_RID_MIN + r % (u32::MAX - MATTER_RID_MIN + 1);
        Ok(Sid(format!("S-1-5-21-{a}-{b}-{c}-{rid}")))
    }
}

/// A canonical decimal: digits only, no sign, no leading zero unless it is `0`, at most 20 digits.
fn decimal(part: &str) -> Option<u64> {
    let canonical = !part.is_empty()
        && part.len() <= 20
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'));
    if canonical { part.parse().ok() } else { None }
}

impl fmt::Display for Sid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Sid {
    type Error = NotASid;

    fn try_from(text: String) -> Result<Sid, NotASid> {
        Sid::parse(&text)
    }
}

impl From<Sid> for String {
    fn from(sid: Sid) -> String {
        sid.0
    }
}

/// The SIDs a matter SID must never be: this machine's real accounts, each named and by its
/// account domain. A matter SID in the domain of the lawyer's or `ClauseoSandbox`'s account
/// (`S-1-5-21-<machine>-…`) could equal a real account of the machine, so every such SID is
/// refused too, as are the accounts themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reserved {
    sids: BTreeSet<Sid>,
    domains: BTreeSet<[u32; 3]>,
}

impl Reserved {
    /// The accounts `sids` and, for each under `S-1-5-21`, its account domain.
    pub fn new(sids: impl IntoIterator<Item = Sid>) -> Reserved {
        let sids: BTreeSet<Sid> = sids.into_iter().collect();
        let domains = sids.iter().filter_map(Sid::domain).collect();
        Reserved { sids, domains }
    }

    /// Whether `sid` may be a matter SID: it is none of the accounts and in none of their
    /// domains.
    pub fn admits(&self, sid: &Sid) -> bool {
        !self.sids.contains(sid)
            && sid
                .domain()
                .is_none_or(|domain| !self.domains.contains(&domain))
    }

    /// This machine's reservations: this process's user, and `ClauseoSandbox` when the
    /// machine's `UserSid` record names it. A read that fails, or a record that is not a SID,
    /// is an error: matter SIDs are not checked against a guess.
    #[cfg(windows)]
    pub fn of_this_machine() -> Result<Reserved, SetupError> {
        let failed = |why: String| SetupError::Failed(format!("this machine's accounts: {why}"));
        let mut sids = vec![super::record::current_user_sid().map_err(|e| failed(e.to_string()))?];
        if let Some(text) = super::record::sandbox_user_sid().map_err(|e| failed(e.to_string()))? {
            sids.push(Sid::parse(&text).map_err(|e| failed(e.to_string()))?);
        }
        Ok(Reserved::new(sids))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_account_domain_is_the_three_parts_after_21() {
        let domain = |text: &str| Sid::parse(text).unwrap().domain();
        assert_eq!(domain("S-1-5-21-7-8-9-1001"), Some([7, 8, 9]));
        assert_eq!(domain("S-1-5-21-7-8-9"), Some([7, 8, 9]));
        assert_eq!(domain("S-1-5-21-7-8"), None);
        assert_eq!(domain("S-1-5-32-544"), None);
        assert_eq!(domain("S-1-5-18"), None);
    }

    #[test]
    fn reserved_refuses_the_accounts_and_their_domains() {
        let lawyer = Sid::parse("S-1-5-21-7-8-9-1001").unwrap();
        let reserved = Reserved::new([lawyer.clone()]);
        assert!(!reserved.admits(&lawyer));
        assert!(!reserved.admits(&Sid::parse("S-1-5-21-7-8-9-1003").unwrap()));
        assert!(!reserved.admits(&Sid::parse("S-1-5-21-7-8-9-600177987").unwrap()));
        assert!(reserved.admits(&Sid::parse("S-1-5-21-7-8-10-1001").unwrap()));
        assert!(Reserved::default().admits(&lawyer));
    }

    #[test]
    fn sids_in_the_canonical_form_parse() {
        for good in [
            "S-1-5-18",
            "S-1-5-32-544",
            "S-1-0-0",
            "S-1-5-21-2626480971-3383066999-180375651-600177987",
            "S-1-15-2-1-2-3-4-5-6-7",
            "S-1-281474976710655-4294967295",
        ] {
            assert_eq!(Sid::parse(good).unwrap().as_str(), good);
        }
    }

    #[test]
    fn anything_else_is_refused() {
        for bad in [
            "",
            "BA",
            "WD",
            "S-1",
            "S-1-5",
            "S-1-5-",
            "S-2-5-18",
            "s-1-5-18",
            "S-1-05-18",
            "S-1-5-018",
            "S-1-5-+18",
            "S-1-5--18",
            "S-1-0x5-18",
            "S-1-5-18 ",
            " S-1-5-18",
            "S-1-5-4294967296",
            "S-1-281474976710656-1",
            "S-1-5-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16",
            "S-1-5-21-1-2-3-१२३४",
        ] {
            assert!(Sid::parse(bad).is_err(), "{bad:?}");
        }
        let json: Result<Sid, _> = serde_json::from_str("\"S-1-5-021\"");
        assert!(json.is_err());
    }

    #[test]
    fn a_new_matter_sid_has_the_matter_form_and_differs_each_time() {
        let mut seen = BTreeSet::new();
        for _ in 0..200 {
            let sid = Sid::random_matter().unwrap();
            assert!(sid.is_matter_form(), "{sid}");
            assert_eq!(Sid::parse(sid.as_str()).unwrap(), sid);
            assert!(seen.insert(sid.clone()), "{sid} came twice");
        }
        assert!(!Sid::parse("S-1-5-21-1-2-3-999").unwrap().is_matter_form());
        assert!(!Sid::parse("S-1-5-32-544").unwrap().is_matter_form());
        assert!(
            !Sid::parse("S-1-5-21-1-2-3-4-1000")
                .unwrap()
                .is_matter_form()
        );
        assert!(!Sid::parse("S-1-16-21-1-2-3-1000").unwrap().is_matter_form());
    }
}
