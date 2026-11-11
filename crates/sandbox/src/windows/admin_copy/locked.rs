//! The admin folder's and copy's locked descriptor ("Admin copy" in 0049): owner
//! Administrators, a protected DACL, full control for SYSTEM and Administrators, read and run
//! for Users, and, once the record names the pool's group (`GroupSid`), a deny of everything
//! for that group, first.
//!
//! Why the deny: Users' read lets any process open the copy, and one that opens it sharing
//! nothing makes every SYSTEM task that runs it (the stop, the refresh) fail to start until it
//! lets go. Every pool user is in `ClauseoSandboxUsers`, and every token a pool user's logon
//! gives, the agent's restricted one included, holds that group (enabled, or deny-only, which a
//! deny entry matches all the same), so no pool process can open either for any access. Users'
//! read stays: the lawyer's unelevated runner and the UAC start read the copy.
//!
//! The group's SID is the machine's own, read from the SYSTEM-written record, never looked up
//! by name. Without a recorded group (the installer's first `--adopt`, before set-up made it)
//! the descriptor has no deny; set-up locks again once it records the group.

use super::{ADMINISTRATORS, FULL_ACCESS, GENERIC_ALL, READ_EXECUTE, SYSTEM, USERS};
use crate::windows::acl_bytes::{self, Acl, Descriptor};
use crate::windows::sids::Sid;

/// The admin folder's SDDL: [`locked_file_sddl`]'s entries, each inherited by files and
/// folders inside.
pub fn locked_folder_sddl(group: Option<&Sid>) -> String {
    sddl(group, "OICI")
}

/// The admin copy's SDDL, written on the file and protected, so it never depends on what it
/// would inherit: with `group`, `(D;;FA;;;<group>)` first, then SYSTEM's and Administrators'
/// full control and Users' read and run.
pub fn locked_file_sddl(group: Option<&Sid>) -> String {
    sddl(group, "")
}

/// The SDDL with `inherit` as each entry's inheritance flags.
fn sddl(group: Option<&Sid>, inherit: &str) -> String {
    let deny = group.map_or_else(String::new, |group| {
        format!("(D;{inherit};FA;;;{})", group.as_str())
    });
    format!("O:BAD:P{deny}(A;{inherit};FA;;;SY)(A;{inherit};FA;;;BA)(A;{inherit};0x1200a9;;;BU)")
}

/// Whether the self-relative descriptor `bytes` is exactly [`locked_folder_sddl`]'s (`folder`)
/// or [`locked_file_sddl`]'s for `group`: owner Administrators, a protected DACL, with `group`
/// its deny entry first, then its three allow entries with those masks and inheritance, in any
/// order, and nothing else. `--adopt`, the set-up and a take-back read what they lock back,
/// and refuse on anything else, a missing deny included.
pub fn locked(bytes: &[u8], folder: bool, group: Option<&Sid>) -> bool {
    let inherit = if folder {
        acl_bytes::OBJECT_INHERIT | acl_bytes::CONTAINER_INHERIT
    } else {
        0
    };
    let owner = acl_bytes::owner_sid(bytes);
    if owner.as_ref().map(Sid::as_str) != Some(ADMINISTRATORS) {
        return false;
    }
    let Ok(descriptor) = Descriptor::parse(bytes) else {
        return false;
    };
    let Some(dacl) = descriptor.dacl.filter(|_| descriptor.dacl_protected()) else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    let mut aces = dacl.aces().iter();
    if let Some(group) = group {
        let first = aces.next().and_then(|ace| read(ace, inherit));
        if first != Some((acl_bytes::DENIED, group.as_str().to_owned(), FULL_ACCESS)) {
            return false;
        }
    }
    let mut seen = Vec::new();
    for ace in aces {
        match read(ace, inherit) {
            Some((acl_bytes::ALLOWED, sid, mask)) => seen.push((sid, mask)),
            _ => return false,
        }
    }
    seen.sort();
    let mut want = vec![
        (SYSTEM.to_owned(), FULL_ACCESS),
        (ADMINISTRATORS.to_owned(), FULL_ACCESS),
        (USERS.to_owned(), READ_EXECUTE),
    ];
    want.sort();
    seen == want
}

/// `ace`'s type, SID and mask when its flags are exactly `inherit`.
fn read(ace: &[u8], inherit: u8) -> Option<(u8, String, u32)> {
    let sid = acl_bytes::sid(ace).and_then(acl_bytes::sid_text)?;
    let mask = acl_bytes::mask(ace)?;
    (acl_bytes::flags(ace) == inherit)
        .then(|| (acl_bytes::kind(ace), sid.as_str().to_owned(), mask))
}

/// [`super::check`]'s rule for the admin folder and the copy once the record names `group`:
/// the self-relative descriptor `bytes` holds a deny entry for `group` that applies to the
/// object itself (not inherit-only) and denies all of [`FULL_ACCESS`] (or `GENERIC_ALL`), and
/// no allow entry comes before it, so Windows reads it first. Anything else reads as not locked
/// (`Blocked::AdminCopyUnsafe`), but a copy locked exactly with no deny is healed first
/// (`heal.rs`); other drift goes to the installer's `--adopt`, which locks it again.
pub fn group_denied(bytes: &[u8], group: &Sid) -> bool {
    let Ok(Descriptor {
        dacl: Some(dacl), ..
    }) = Descriptor::parse(bytes)
    else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    for ace in dacl.aces() {
        let kind = acl_bytes::kind(ace);
        if matches!(kind, 0 | 4 | 5 | 9 | 11) {
            return false;
        }
        let applies = acl_bytes::flags(ace) & acl_bytes::INHERIT_ONLY == 0;
        let all = acl_bytes::mask(ace)
            .is_some_and(|mask| mask & FULL_ACCESS == FULL_ACCESS || mask & GENERIC_ALL != 0);
        let named = acl_bytes::sid(ace)
            .and_then(acl_bytes::sid_text)
            .is_some_and(|sid| sid == *group);
        if kind == acl_bytes::DENIED && applies && all && named {
            return true;
        }
    }
    false
}
