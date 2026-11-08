//! How the sweep reads a principal's name ([`resolve`]) and a task's owner ([`sddl_owner`]): a
//! SID's text in any form Windows takes ([`canonical_sid`]), the Task Scheduler's names for the
//! service accounts, a name the local lookup resolves, and the rules for a name it can't
//! ([`Names`]).

use super::{Failed, Sweeper};
use crate::windows::sids::Sid;

/// The Task Scheduler's own names for the service accounts, with their SIDs.
const ALIASES: [(&str, &str); 3] = [
    ("LocalSystem", "S-1-5-18"),
    ("LocalService", "S-1-5-19"),
    ("NetworkService", "S-1-5-20"),
];

/// What a principal's name resolves to ([`resolve`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// The SID it is, or names.
    Sid(Sid),
    /// No SID, but its account part is a pool user's name ([`Names::is_pool_name`]).
    PoolName,
    /// No SID, and another domain's name ([`Names::is_foreign`]): skipped. Why the lookup
    /// failed.
    Skipped(Failed),
    /// No SID, and neither another domain's name nor a pool user's: it may be the pool's, so
    /// the task is left and the run ends 56. Why the lookup failed.
    Unresolved(Failed),
}

/// The names an unresolved principal is held to: each pool user's account name, and this
/// computer's names, its only accepted prefixes besides `.`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    /// The pool users' account names (`ClauseoSbNN`), those that could be read.
    pub users: Vec<String>,
    /// This computer's `NetBIOS` name and DNS host name (`GetComputerNameExW`), those that
    /// could be read.
    pub computer: Vec<String>,
}

impl Names {
    /// The names for `pool`, read once a run; a pool user whose name can't be read is still
    /// matched by its SID.
    pub fn of(fs: &mut impl Sweeper, pool: &[Sid]) -> Names {
        let users = pool
            .iter()
            .filter_map(|sid| fs.account_name(sid).ok())
            .collect();
        Names {
            users,
            computer: fs.computer_names(),
        }
    }

    /// Whether `name`'s account part is a pool user's name, without case, and it has no
    /// prefix, or `.`, or one of this computer's names: `ClauseoSb01`, `.\ClauseoSb01`,
    /// `PC-7\clauseosb01`, never `CORP\ClauseoSb01`.
    pub fn is_pool_name(&self, name: &str) -> bool {
        let (local, account) = match name.rsplit_once('\\') {
            None => (true, name),
            Some((prefix, account)) => {
                let here = prefix == "."
                    || self
                        .computer
                        .iter()
                        .any(|computer| computer.eq_ignore_ascii_case(prefix));
                (here, account)
            }
        };
        local
            && !account.is_empty()
            && self
                .users
                .iter()
                .any(|user| user.eq_ignore_ascii_case(account))
    }

    /// Whether `name` is `DOMAIN\account` for another domain: one prefix, not empty, not `.`
    /// and none of this computer's names (without case), and an account part. With no
    /// computer name read, no name is another domain's.
    pub fn is_foreign(&self, name: &str) -> bool {
        let Some((prefix, account)) = name.split_once('\\') else {
            return false;
        };
        let lower = prefix.to_lowercase();
        !self.computer.is_empty()
            && !prefix.is_empty()
            && prefix != "."
            && !account.is_empty()
            && !account.contains('\\')
            && !self
                .computer
                .iter()
                .any(|computer| computer.to_lowercase() == lower)
    }
}

/// What a principal's `name` resolves to: a SID's text is that SID ([`canonical_sid`]); the
/// Task Scheduler's names for the service accounts ([`ALIASES`], any case) are theirs; any other
/// name is looked up ([`Sweeper::lookup_account`]). A name the lookup can't resolve is
/// [`Resolved::PoolName`] when `names` holds it ([`Names::is_pool_name`]), skipped when it is
/// another domain's ([`Names::is_foreign`]), and [`Resolved::Unresolved`] otherwise.
pub fn resolve(fs: &mut impl Sweeper, names: &Names, name: &str) -> Resolved {
    if let Some(sid) = canonical_sid(name) {
        return Resolved::Sid(sid);
    }
    let alias = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name));
    if let Some((_, sid)) = alias {
        return Sid::parse(sid).map_or_else(
            |error| Resolved::Unresolved(error.to_string()),
            Resolved::Sid,
        );
    }
    match fs.lookup_account(name) {
        Ok(sid) => Resolved::Sid(sid),
        Err(_) if names.is_pool_name(name) => Resolved::PoolName,
        Err(why) if names.is_foreign(name) => Resolved::Skipped(why),
        Err(why) => Resolved::Unresolved(why),
    }
}

/// `text` as a [`Sid`] when it is a SID's text in a form `ConvertStringSidToSidW` takes: `S-`
/// or `s-`, revision 1, an authority in decimal or in hex after `0x` (48 bits at most), and 1
/// to 15 decimal sub-authorities, each a `u32`; leading zeros allowed. `None` for anything
/// else, aliases such as `SY` included.
pub fn canonical_sid(text: &str) -> Option<Sid> {
    let rest = text
        .strip_prefix("S-")
        .or_else(|| text.strip_prefix("s-"))?;
    let mut parts = rest.split('-');
    if number(parts.next()?, 10)? != 1 {
        return None;
    }
    let authority = parts.next()?;
    let authority = match authority
        .strip_prefix("0x")
        .or_else(|| authority.strip_prefix("0X"))
    {
        Some(hex) => number(hex, 16)?,
        None => number(authority, 10)?,
    };
    if authority >= 1 << 48 {
        return None;
    }
    let mut canonical = vec![format!("S-1-{authority}")];
    for part in parts {
        let sub = u32::try_from(number(part, 10)?).ok()?;
        canonical.push(sub.to_string());
    }
    if !(2..=16).contains(&canonical.len()) {
        return None;
    }
    Sid::parse(&canonical.join("-")).ok()
}

/// `digits` in `radix`: 1 to 20 digits of it and nothing else, fitting a `u64`.
fn number(digits: &str, radix: u32) -> Option<u64> {
    let plain = (1..=20).contains(&digits.len()) && digits.chars().all(|c| c.is_digit(radix));
    if plain {
        u64::from_str_radix(digits, radix).ok()
    } else {
        None
    }
}

/// The owner a task's security descriptor names, from its SDDL (`O:` first, up to the next
/// `G:`, `D:` or `S:`): `Ok(Some)` for a SID's text ([`canonical_sid`]), `Ok(None)` for a
/// two-letter alias (`SY`, `BA`: never a pool user), `Err` for no owner or anything else.
pub fn sddl_owner(sddl: &str) -> Result<Option<Sid>, Failed> {
    let malformed = || "the task's security descriptor names no owner it can read".to_owned();
    let rest = sddl.strip_prefix("O:").ok_or_else(malformed)?;
    let end = ["G:", "D:", "S:"]
        .iter()
        .filter_map(|next| rest.find(next))
        .min()
        .unwrap_or(rest.len());
    let owner = rest.get(..end).ok_or_else(malformed)?;
    if let Some(sid) = canonical_sid(owner) {
        return Ok(Some(sid));
    }
    if owner.len() == 2 && owner.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Ok(None);
    }
    Err(malformed())
}
