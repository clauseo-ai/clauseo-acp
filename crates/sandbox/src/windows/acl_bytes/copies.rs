//! Which explicit entries of a child are copies of inherited ones (S15 PR3 round 8). Pure, so it
//! is tested on every OS.

use super::{Acl, INHERITED, flags};

/// Whether `ace` copies an inherited entry: equal, all but its inherited flag, to an inherited
/// entry of `due` (what the parent gives the child now) or of `held` (the child's own DACL).
/// Type, mask, SID and inheritance flags must match: on a file a copy has its flags cleared, as
/// the parent's object-inherit entries reach a file; on a folder it has the flags an entry
/// inherited onto a container has. Entries `due` holds that aren't marked inherited (a default
/// DACL's, when the parent gives nothing) don't count.
pub(crate) fn is_copy(ace: &[u8], due: &Acl<'_>, held: &Acl<'_>) -> bool {
    let same = |other: &&[u8]| {
        flags(other) & INHERITED != 0
            && other.len() == ace.len()
            && other.first() == ace.first()
            && flags(other) & !INHERITED == flags(ace) & !INHERITED
            && other.get(2..) == ace.get(2..)
    };
    due.aces().iter().chain(held.aces()).any(same)
}
