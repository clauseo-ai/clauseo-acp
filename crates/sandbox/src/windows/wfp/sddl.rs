//! The security descriptors tier B's filter conditions name, and their parsing back from the
//! SDDL WFP returns (S15 PR7), with no Windows call.
//!
//! A block filter matches with `FWP_MATCH_NOT_EQUAL` over [`not_pool_sddl`]: WFP's condition is
//! true when an access check of the socket's token does *not* grant `FWP_ACTRL_MATCH_FILTER`. A
//! deny entry also matches a deny-only SID, so a pool process that turns its own user SID (or
//! the group) deny-only in a derived token is still denied, and still blocked (the review's
//! item 1). The allows keep everyone else out of the block: `Everyone`, `RESTRICTED` for a
//! restricted token's second pass, and the two application-package groups for `AppContainer` and
//! LPAC apps. A static permit matches with `FWP_MATCH_EQUAL` over [`user_sddl`], one allow: a
//! derived token that escapes the allow loses the permit and falls to the block.

use crate::windows::sids::Sid;

/// The allows after a block descriptor's denies, in order: `Everyone`, `RESTRICTED`, `ALL
/// APPLICATION PACKAGES`, `ALL RESTRICTED APPLICATION PACKAGES`. Each with the SDDL alias Windows
/// may print for it.
const BLOCK_ALLOWS: [(&str, &str); 4] = [
    ("WD", "S-1-1-0"),
    ("RC", "S-1-5-12"),
    ("AC", "S-1-15-2-1"),
    ("S-1-15-2-2", "S-1-15-2-2"),
];

/// One allow (`A`) or deny (`D`) of `CC` per SID, in SDDL.
pub(super) fn aces(kind: char, sids: &[Sid]) -> String {
    sids.iter()
        .map(|sid| format!("({kind};;CC;;;{sid})"))
        .collect::<Vec<_>>()
        .concat()
}

/// The descriptor a permit's `ALE_USER_ID` condition names: owner and group `SYSTEM`, one
/// allow of `FWP_ACTRL_MATCH_FILTER` (`CC`) per SID, as PR0 (c) measured it.
pub fn user_sddl(sids: &[Sid]) -> String {
    format!("O:SYG:SYD:{}", aces('A', sids))
}

/// The descriptor a block's `ALE_USER_ID` condition names, matched `NOT_EQUAL`: a deny of `CC`
/// for the pool group and each pool user, then [`BLOCK_ALLOWS`].
pub fn not_pool_sddl(group: &Sid, pool: &[Sid]) -> String {
    let allows: String = BLOCK_ALLOWS
        .iter()
        .map(|(alias, _)| format!("(A;;CC;;;{alias})"))
        .collect::<Vec<_>>()
        .concat();
    format!(
        "O:SYG:SYD:{}{}{allows}",
        aces('D', std::slice::from_ref(group)),
        aces('D', pool)
    )
}

/// The DACL's entries as (kind, SID text), each exactly `(<A|D>;;CC;;;<sid>)`; `None` for any
/// other entry, flags or rights, or no DACL.
fn dacl(sddl: &str) -> Option<Vec<(char, &str)>> {
    let (_, dacl) = sddl.split_once("D:")?;
    let dacl = dacl.split("S:").next().unwrap_or(dacl);
    let (flags, rest) = dacl.split_once('(')?;
    if !flags.is_empty() {
        return None;
    }
    let mut entries = Vec::new();
    for ace in rest.split_terminator(")(") {
        let ace = ace.strip_suffix(')').unwrap_or(ace);
        match ace.split(';').collect::<Vec<_>>().as_slice() {
            ["A", "", "CC", "", "", sid] => entries.push(('A', *sid)),
            ["D", "", "CC", "", "", sid] => entries.push(('D', *sid)),
            _ => return None,
        }
    }
    (!entries.is_empty()).then_some(entries)
}

/// The SIDs a permit's descriptor allows, read from SDDL: every entry an allow of exactly `CC`
/// for a SID in full form. `None` for anything else (a deny, other rights, a SID in alias form
/// such as `WD`), so no check reads coverage out of a descriptor it doesn't understand.
pub fn allowed_users(sddl: &str) -> Option<Vec<Sid>> {
    dacl(sddl)?
        .into_iter()
        .map(|(kind, sid)| (kind == 'A').then(|| Sid::parse(sid).ok()).flatten())
        .collect()
}

/// The SIDs a block's descriptor denies, read from SDDL: denies of `CC` for SIDs in full form,
/// then exactly [`BLOCK_ALLOWS`] (by alias or SID). `None` for any other shape.
pub fn denied_users(sddl: &str) -> Option<Vec<Sid>> {
    let entries = dacl(sddl)?;
    let split = entries.iter().position(|(kind, _)| *kind == 'A')?;
    let (denies, allows) = entries.split_at(split);
    let allows_ok = allows.len() == BLOCK_ALLOWS.len()
        && allows
            .iter()
            .zip(BLOCK_ALLOWS)
            .all(|((kind, sid), (alias, full))| *kind == 'A' && (*sid == alias || *sid == full));
    if !allows_ok || denies.is_empty() {
        return None;
    }
    denies.iter().map(|(_, sid)| Sid::parse(sid).ok()).collect()
}
