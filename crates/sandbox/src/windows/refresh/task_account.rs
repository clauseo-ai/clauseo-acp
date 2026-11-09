//! A task's principal and its logon trigger's user against the SID the set-up registered, for
//! `task_identity.rs`, with no domain controller in the way. The Task Scheduler gives either back
//! as a SID or as a name: on the VM (Server 2025) the principal read as `SYSTEM` through COM, and
//! the refresh task's logon trigger, registered by SID, as `MACHINE\user`. A SID is compared as a
//! SID. A name is compared with the expected SID's own name, looked up locally
//! (`LookupAccountSidW`, answered from well-known SIDs, the local accounts and the LSA's cache of
//! logged-on users), without case, its prefix `.`, none, or the domain that lookup names. The
//! name is never resolved forwards (`LookupAccountNameW`), which may ask a domain controller. A
//! lookup that fails can't tell either way: couldn't-check, never changed. No Windows call here;
//! tested on every OS.

use crate::windows::sids::Sid;

/// An account's name as `LookupAccountSidW` gives it: its name, then its domain.
pub type AccountName = (String, String);

/// The local lookup of the account a SID names: `Ok(None)` when no account has it.
pub type NameOf<'a> = dyn FnMut(&Sid) -> Result<Option<AccountName>, String> + 'a;

/// LOCAL SYSTEM.
const SYSTEM: &str = "S-1-5-18";

/// LOCAL SYSTEM's names as an English Windows and the Task Scheduler give them back, matched
/// without a lookup, so a launch's check of the stop and sweep tasks usually makes none. Another
/// language's name is looked up (`NameOf`), as LOCAL SYSTEM, a well-known SID: answered on the
/// machine, never by a domain controller.
const SYSTEM_NAMES: [&str; 4] = ["LocalSystem", "SYSTEM", r"NT AUTHORITY\SYSTEM", r".\SYSTEM"];

/// A principal or logon trigger user as read back: a SID's text in its canonical form, else the
/// name as written, trimmed.
pub fn account_text(read: &str) -> String {
    let read = read.trim();
    Sid::parse(read).map_or_else(|_not_a_sid| read.to_owned(), |sid| sid.as_str().to_owned())
}

/// Whether `found` (as [`account_text`] gives it) names the account `expected` (a SID's text)
/// does: the same SID, or a name that is the expected SID's local name ([`NameOf`]), without
/// case, bare, after `.\`, or after the domain the lookup names. LOCAL SYSTEM's English names
/// ([`SYSTEM_NAMES`], the Task Scheduler's own `LocalSystem` included) need no lookup. `Err`
/// when the expected SID's name can't be looked up.
pub fn same_account(found: &str, expected: &str, name_of: &mut NameOf<'_>) -> Result<bool, String> {
    let Ok(want) = Sid::parse(expected.trim()) else {
        return Ok(false);
    };
    let found = found.trim();
    if let Ok(sid) = Sid::parse(found) {
        return Ok(sid.as_str() == want.as_str());
    }
    if found.is_empty() {
        return Ok(false);
    }
    if want.as_str() == SYSTEM && SYSTEM_NAMES.iter().any(|n| found.eq_ignore_ascii_case(n)) {
        return Ok(true);
    }
    let (prefix, account) = match found.rsplit_once('\\') {
        Some((prefix, account)) => (Some(prefix), account),
        None => (None, found),
    };
    let Some((name, domain)) = name_of(&want)? else {
        return Err(format!("no local account name for {}", want.as_str()));
    };
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    Ok(same(account, &name) && prefix.is_none_or(|prefix| prefix == "." || same(prefix, &domain)))
}
