//! What the walk decides about one child before it touches it (S15 `PR3b` round 2): which writes
//! it needs and in what order, and which of them its handle has no right for (N3); and, after a
//! hand-back, whether what it reads back is handed back ([`handed_back`], S15 `PR4h`); and whether
//! what the elevated clean-up took, cleaned of other pool users' allows ([`stripped`], S15 `PR4i`).
//! Pure, so it is tested on every OS.

use std::io;

use super::{
    ALLOWED, ALLOWED_CALLBACK, Acl, DACL_AUTO_INHERITED, DACL_PROTECTED, DENIED, DENIED_CALLBACK,
    Descriptor, INHERITED, broad_allow, build_descriptor, empty_acl, flags, kind, owner_bytes,
    rebuild, render, sid,
};

/// One write on a child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Write {
    /// Its DACL: inherited entries worked out again, cleaned when repaired or handed back.
    Dacl,
    /// Its owner: the hand-back's.
    Owner,
    /// Its mandatory label.
    Label,
}

impl Write {
    /// Whether it needs `WRITE_OWNER` (an owner or a label) rather than `WRITE_DAC`.
    fn needs_write_owner(self) -> bool {
        matches!(self, Write::Owner | Write::Label)
    }
}

/// The writes a child needs, in the order they are made: the owner, then the DACL, then the
/// label (S15 `PR4h`). The owner goes first, so once the cleaned DACL strips a pool user's
/// allows, that user owns the child no more and can't put one back through an owner's implied
/// `WRITE_DAC`; the hand-back is read again after (`handed`). A failure between the two leaves a
/// child the lawyer owns with the agent's allows still on it: the next strict walk refuses it as
/// [`BroadAllowChild`](crate::windows::writer::ApplyFailure::BroadAllowChild), fail closed. (N13
/// wrote the DACL first, which left the pool user owning the child in between.)
pub(crate) fn writes(dacl: bool, owner: bool, label: bool) -> Vec<Write> {
    [
        (owner, Write::Owner),
        (dacl, Write::Dacl),
        (label, Write::Label),
    ]
    .into_iter()
    .filter_map(|(needed, write)| needed.then_some(write))
    .collect()
}

/// The first of `writes` that a handle opened with `write_dac` and `write_owner` can't make.
/// Checked before any write, so a child is never left half done (N3).
pub(crate) fn lacking(writes: &[Write], write_dac: bool, write_owner: bool) -> Option<Write> {
    writes.iter().copied().find(|write| {
        if write.needs_write_owner() {
            !write_owner
        } else {
            !write_dac
        }
    })
}

/// Whether a hand-back's descriptor, read back through its handle (`after`), is handed back:
/// its owner is `me` (the desktop user), and its DACL holds no explicit allow for a pool SID of
/// `pool` but `own` (the matter's own subjects), nor for a broad group ([`broad_allow`]).
/// Inherited entries don't count. `false` refuses the item (S15 `PR4h`).
pub(crate) fn handed_back(
    after: &[u8],
    pool: &[Vec<u8>],
    own: &[Vec<u8>],
    me: &[u8],
) -> io::Result<bool> {
    let others: Vec<Vec<u8>> = pool.iter().filter(|p| !own.contains(p)).cloned().collect();
    let dacl = Descriptor::parse(after)?.dacl.map(Acl::parse).transpose()?;
    let allowed = dacl.as_ref().is_some_and(|d| broad_allow(d, &others));
    Ok(owner_bytes(after) == Some(me) && !allowed)
}

/// What `--reclaim` took, cleaned (S15 `PR4i`): when `descriptor` is an item the desktop user
/// (`me`) owns whose DACL holds an explicit plain allow for one of `others` (the pool users but
/// the matter's own), or an explicit deny for one of `mine` that names none of `others` (`mine`
/// is empty but on an item `--reclaim` took: there, the desktop user, its token's groups and the
/// owner's stand-ins, less the pool and its group), the descriptor to write: its
/// DACL without exactly those entries, every other entry kept (a broad group's allow, the
/// matter's own pool user's, inherited ones, a deny naming a pool user or the pool group: our
/// fence) and its control bits as they were, protection included. So an agent can't leave on
/// what it made a deny that later refuses the lawyer a write, such as its label. In a protected
/// DACL an entry marked inherited counts as explicit: nothing can be inherited there. `None`
/// when there is nothing to strip.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the hand-back after --reclaim is Windows only")
)]
pub(crate) fn stripped(
    descriptor: &[u8],
    others: &[Vec<u8>],
    me: &[u8],
    mine: &[Vec<u8>],
) -> io::Result<Option<Vec<u8>>> {
    if me.is_empty() || owner_bytes(descriptor) != Some(me) {
        return Ok(None);
    }
    let now = Descriptor::parse(descriptor)?;
    let Some(dacl) = now.dacl else {
        return Ok(None);
    };
    let dacl = Acl::parse(dacl)?;
    // Nothing is inherited through a protected DACL: an entry there marked inherited is one an
    // agent forged (`D:P(A;ID;FA;;;<pool user>)`), and goes like an explicit one.
    let protected = now.dacl_protected();
    let offends =
        |ace: &&[u8]| (protected || flags(ace) & INHERITED == 0) && offending(ace, others, mine);
    if !dacl.aces().iter().any(offends) {
        return Ok(None);
    }
    let kept: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|a| !offends(a))
        .collect();
    let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
    let new = render(dacl.revision.max(2), &kept);
    Ok(Some(build_descriptor(now.control & bits, Some(&new), None)))
}

/// Whether `ace` is one the strip takes off when explicit ([`stripped`]): an allow for one of
/// `others`, or a deny for one of `mine` that names none of `others`. A deny object entry
/// counts as a plain deny: without an object type, an access check applies it as one, and with
/// one it denies nothing a file's check asks; either way the lawyer loses nothing by its going.
fn offending(ace: &[u8], others: &[Vec<u8>], mine: &[Vec<u8>]) -> bool {
    let named = |found: Option<&[u8]>, set: &[Vec<u8>]| {
        found.is_some_and(|s| set.iter().any(|o| o.as_slice() == s))
    };
    match kind(ace) {
        ALLOWED | ALLOWED_CALLBACK => named(sid(ace), others),
        DENIED | DENIED_CALLBACK => named(sid(ace), mine) && !named(sid(ace), others),
        DENIED_OBJECT | DENIED_CALLBACK_OBJECT => {
            named(object_sid(ace), mine) && !named(object_sid(ace), others)
        }
        _ => false,
    }
}

/// What `--reclaim` writes on an item it takes, before its owner (S15 `PR4i` follow-up): the DACL
/// without every entry naming `OWNER RIGHTS` (S-1-3-4), allow or deny, explicit or marked
/// inherited, of any type that names a SID; every other entry byte for byte, in its order, and
/// the DACL's revision and control bits as they were. An `OWNER RIGHTS` entry takes the owner's
/// implied `READ_CONTROL | WRITE_DAC` away, so with one the lawyer couldn't open the item even
/// once it owns it. `None` when there is none, or no DACL. Built for Windows and the tests.
#[cfg(any(windows, test))]
pub(crate) fn without_owner_rights(descriptor: &[u8]) -> io::Result<Option<Vec<u8>>> {
    /// `OWNER RIGHTS`, as bytes: revision 1, one sub-authority, authority 3, RID 4.
    const OWNER_RIGHTS: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 3, 4, 0, 0, 0];
    let now = Descriptor::parse(descriptor)?;
    let Some(dacl) = now.dacl else {
        return Ok(None);
    };
    let dacl = Acl::parse(dacl)?;
    let names_it = |ace: &&[u8]| any_sid(ace) == Some(OWNER_RIGHTS.as_slice());
    if !dacl.aces().iter().any(names_it) {
        return Ok(None);
    }
    let kept: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|ace| !names_it(ace))
        .collect();
    let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
    let new = render(dacl.revision, &kept);
    Ok(Some(build_descriptor(now.control & bits, Some(&new), None)))
}

/// An entry's SID, for every type that names one: the plain, callback and label types, and the
/// object types (allow and deny, plain and callback).
#[cfg(any(windows, test))]
fn any_sid(ace: &[u8]) -> Option<&[u8]> {
    match kind(ace) {
        5 | DENIED_OBJECT | 11 | DENIED_CALLBACK_OBJECT => object_sid(ace),
        _ => sid(ace),
    }
}

/// `ACCESS_DENIED_OBJECT_ACE_TYPE`.
const DENIED_OBJECT: u8 = 6;
/// `ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE`.
const DENIED_CALLBACK_OBJECT: u8 = 12;

/// An object entry's SID: after its mask, its flags, and the object type and inherited object
/// type GUIDs its flags say are present.
fn object_sid(ace: &[u8]) -> Option<&[u8]> {
    let flags = u32::from_le_bytes(ace.get(8..12)?.try_into().ok()?);
    let mut at = 12;
    for present in [0x1, 0x2] {
        if flags & present != 0 {
            at += 16;
        }
    }
    let body = ace.get(at..)?;
    let count = usize::from(*body.get(1)?);
    body.get(..8 + 4 * count)
}

/// Whether `descriptor` (what [`stripped`] made of an item `--reclaim` took) is left with a
/// protected DACL and no explicit entry: the agent's protection, with nothing of anyone else's
/// under it. The hand-back after then unprotects it, so it inherits the matter's permissions
/// (S15 `PR4i`). Any entry left (a colleague's, the lawyer's, a deny), even one marked
/// inherited, which a protected DACL can't really hold, keeps it as it is.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the hand-back after --reclaim is Windows only")
)]
pub(crate) fn bare(descriptor: &[u8]) -> io::Result<bool> {
    let now = Descriptor::parse(descriptor)?;
    let Some(dacl) = now.dacl else {
        return Ok(false);
    };
    Ok(now.dacl_protected() && Acl::parse(dacl)?.aces().is_empty())
}

/// The one write the hand-back after `--reclaim` makes on a taken item (S15 `PR4i`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TakenWrite {
    /// Its DACL without the other pool users' allows and the denies of the desktop user's
    /// token, protection kept ([`stripped`]).
    Stripped(Vec<u8>),
    /// Unprotected, its explicit entries (less those) and the inherited entries its folder
    /// gives now, and nothing else: an item the strip left [`bare`], or one whose inherited
    /// entries still let another pool user in or deny the desktop user's token (copies of the
    /// agent's entries on what was below it).
    Inheriting(Vec<u8>),
}

/// What to write on the taken item `current`, in one write. The other pool users' allows and the
/// denies of `mine` come off first ([`stripped`]); then, when that leaves it [`bare`], or
/// unprotected with an inherited entry still of those, the inherited entries are worked out by
/// `inherit` from the stripped item's explicit entries alone. So the inherited grants are worked
/// out only once the pool users' allows are gone, and the item never holds both. `None` when
/// there is nothing to change.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the hand-back after --reclaim is Windows only")
)]
pub(crate) fn taken_write(
    current: &[u8],
    others: &[Vec<u8>],
    me: &[u8],
    mine: &[Vec<u8>],
    inherit: &mut dyn FnMut(&[u8]) -> io::Result<Vec<u8>>,
) -> io::Result<Option<TakenWrite>> {
    if me.is_empty() || owner_bytes(current) != Some(me) {
        return Ok(None);
    }
    let stripped_now = stripped(current, others, me, mine)?;
    let after = stripped_now.as_deref().unwrap_or(current);
    let read = Descriptor::parse(after)?;
    let Some(dacl) = read.dacl else {
        return Ok(None);
    };
    let dacl = Acl::parse(dacl)?;
    let stale = !read.dacl_protected()
        && dacl
            .aces()
            .iter()
            .any(|ace| flags(ace) & INHERITED != 0 && offending(ace, others, mine));
    if !bare(after)? && !stale {
        return Ok(stripped_now.map(TakenWrite::Stripped));
    }
    let explicit: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|ace| flags(ace) & INHERITED == 0)
        .collect();
    let base = rebuild(
        after,
        None,
        Some(&render(dacl.revision.max(2), &explicit)),
        DACL_PROTECTED,
    )?;
    let worked = inherit(&base)?;
    let new = Descriptor::parse(&worked)?
        .dacl
        .map_or_else(empty_acl, <[u8]>::to_vec);
    Ok(Some(TakenWrite::Inheriting(build_descriptor(
        DACL_AUTO_INHERITED,
        Some(&new),
        None,
    ))))
}
