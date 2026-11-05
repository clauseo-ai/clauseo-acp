// SPDX-License-Identifier: GPL-3.0-or-later AND Apache-2.0
// Derived from openai/codex codex-rs/windows-sandbox-rs/src/acl.rs
// at 092d3acd6bec3e3a14bdc7e7a2810ab628ab759d (rust-v0.162.1); see PROVENANCE.md
//! Security descriptors, ACLs and ACEs as bytes (S15 PR3). Pure: the walk (`acl.rs`) reads a
//! file's self-relative security descriptor through Win32 and hands it here to find, add or
//! remove one entry, to work out the Low label a child inherits, and to look for what beats an
//! inherited deny. Built and tested on every OS.
//!
//! Layouts, all little-endian: `SECURITY_DESCRIPTOR_RELATIVE` (revision, control, then the
//! offsets of owner, group, SACL and DACL), `ACL` (revision, size, count, then the entries), and
//! the `ACCESS_ALLOWED_ACE`, `ACCESS_DENIED_ACE` and `SYSTEM_MANDATORY_LABEL_ACE` bodies (a
//! header, a mask, then the SID). The scans follow Codex's `acl.rs` (`dacl_has_deny_mask` and the
//! allow checks; see `PROVENANCE.md`), rewritten over bytes so no entry is ever merged into
//! another: `SetEntriesInAclW` folds a new allow into the trustee's existing one, which would
//! leave nothing exact to remove later.

use std::io;

use super::acl_plan::{Grant, Inherit};
use super::sids::Sid;
use super::writer::Label;

/// `ACCESS_ALLOWED_ACE_TYPE`.
pub(crate) const ALLOWED: u8 = 0;
/// `ACCESS_ALLOWED_CALLBACK_ACE_TYPE`.
const ALLOWED_CALLBACK: u8 = 9;
/// `ACCESS_DENIED_ACE_TYPE`.
pub(crate) const DENIED: u8 = 1;
/// `SYSTEM_MANDATORY_LABEL_ACE_TYPE`.
pub(crate) const LABEL: u8 = 0x11;
/// `OBJECT_INHERIT_ACE`.
pub(crate) const OBJECT_INHERIT: u8 = 0x1;
/// `CONTAINER_INHERIT_ACE`.
pub(crate) const CONTAINER_INHERIT: u8 = 0x2;
/// `NO_PROPAGATE_INHERIT_ACE`.
pub(crate) const NO_PROPAGATE: u8 = 0x4;
/// `INHERIT_ONLY_ACE`.
pub(crate) const INHERIT_ONLY: u8 = 0x8;
/// `INHERITED_ACE`.
pub(crate) const INHERITED: u8 = 0x10;

/// `SE_DACL_PRESENT`.
const DACL_PRESENT: u16 = 0x0004;
/// `SE_SACL_PRESENT`.
const SACL_PRESENT: u16 = 0x0010;
/// `SE_DACL_AUTO_INHERIT_REQ`: asks `SetKernelObjectSecurity` to keep
/// `SE_DACL_AUTO_INHERITED`, which it drops otherwise.
const DACL_AUTO_INHERIT_REQ: u16 = 0x0100;
/// `SE_DACL_AUTO_INHERITED`.
pub(crate) const DACL_AUTO_INHERITED: u16 = 0x0400;
/// `SE_SACL_AUTO_INHERITED`.
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "the label writes are Windows only")
)]
pub(crate) const SACL_AUTO_INHERITED: u16 = 0x0800;
/// `SE_DACL_PROTECTED`.
pub(crate) const DACL_PROTECTED: u16 = 0x1000;
/// `SE_SACL_PROTECTED`.
pub(crate) const SACL_PROTECTED: u16 = 0x2000;
/// `SE_SELF_RELATIVE`.
const SELF_RELATIVE: u16 = 0x8000;

/// The relative id of a machine's `None` group (`DOMAIN_GROUP_RID_USERS`).
const NONE_RID: u32 = 513;

/// `SYNCHRONIZE`: the one right an allow may give and still not count as access.
const SYNCHRONIZE: u32 = 0x0010_0000;

/// The groups a pool user's token holds, as Windows gives a local interactive account, and
/// `RESTRICTED`: an explicit allow naming one, on a child, is checked before an inherited deny,
/// so it beats the fence and the siblings' deny (see [`broad_allow`]).
pub(crate) const BROAD_SIDS: [&str; 10] = [
    "S-1-1-0",      // Everyone
    "S-1-2-0",      // LOCAL
    "S-1-2-1",      // CONSOLE LOGON
    "S-1-5-4",      // INTERACTIVE
    "S-1-5-11",     // Authenticated Users
    "S-1-5-12",     // RESTRICTED
    "S-1-5-15",     // This Organization
    "S-1-5-32-545", // Users
    "S-1-5-64-10",  // NTLM Authentication
    "S-1-5-113",    // Local account
];

/// The bytes of `sid`: revision 1, the count, the authority (six bytes, big-endian), then each
/// sub-authority (four bytes, little-endian). [`Sid`] has checked the form already.
pub(crate) fn sid_bytes(sid: &Sid) -> Vec<u8> {
    let mut parts = sid.as_str().split('-').skip(2);
    let authority: u64 = parts.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    let subs: Vec<u32> = parts.filter_map(|p| p.parse().ok()).collect();
    let mut out = vec![1, u8::try_from(subs.len()).unwrap_or(0)];
    out.extend_from_slice(authority.to_be_bytes().get(2..).unwrap_or(&[0; 6]));
    for sub in subs {
        out.extend_from_slice(&sub.to_le_bytes());
    }
    out
}

/// The SID whose bytes start `bytes`, in [`Sid`]'s text form; `None` when they aren't one.
pub(crate) fn sid_text(bytes: &[u8]) -> Option<Sid> {
    let count = usize::from(*bytes.get(1)?);
    let mut authority = [0u8; 8];
    authority.get_mut(2..)?.copy_from_slice(bytes.get(2..8)?);
    let mut text = format!("S-{}-{}", bytes.first()?, u64::from_be_bytes(authority));
    for at in 0..count {
        let sub = read_u32(bytes, 8 + 4 * at)?;
        text.push('-');
        text.push_str(&sub.to_string());
    }
    Sid::parse(&text).ok()
}

/// One entry to find, add or remove: type, flags, mask and SID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AceSpec {
    pub(crate) kind: u8,
    pub(crate) flags: u8,
    pub(crate) mask: u32,
    pub(crate) sid: Vec<u8>,
}

impl AceSpec {
    /// The entry `grant` writes on a folder (`folder`) or a file. On a file a [`Inherit::Tree`]
    /// entry inherits nothing (files have no children); [`Inherit::FilesInFolder`] is for a
    /// folder only.
    pub(crate) fn for_grant(grant: &Grant, folder: bool) -> io::Result<AceSpec> {
        let rights = grant.rights();
        let kind = if rights.is_label() {
            LABEL
        } else if rights.is_deny() {
            DENIED
        } else {
            ALLOWED
        };
        let flags = match (grant.inherit(), folder) {
            (Inherit::ThisOnly, _) | (Inherit::Tree, false) => 0,
            (Inherit::Tree, true) => OBJECT_INHERIT | CONTAINER_INHERIT,
            (Inherit::FilesInFolder, true) => OBJECT_INHERIT | INHERIT_ONLY | NO_PROPAGATE,
            (Inherit::FilesInFolder, false) => {
                return Err(invalid("an entry for the files in a folder is on a file"));
            }
        };
        Ok(AceSpec {
            kind,
            flags,
            mask: rights.mask(),
            sid: sid_bytes(grant.sid()),
        })
    }

    /// The entry's bytes.
    pub(crate) fn bytes(&self) -> Vec<u8> {
        let size = 8 + self.sid.len();
        let mut out = Vec::with_capacity(size);
        out.push(self.kind);
        out.push(self.flags);
        out.extend_from_slice(&u16::try_from(size).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&self.mask.to_le_bytes());
        out.extend_from_slice(&self.sid);
        out
    }

    /// Whether `ace` is this entry, written explicitly.
    fn is(&self, ace: &[u8]) -> bool {
        kind(ace) == self.kind
            && flags(ace) == self.flags
            && mask(ace) == Some(self.mask)
            && sid(ace) == Some(self.sid.as_slice())
    }
}

/// An ACL: its revision and its entries, each the whole entry's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Acl<'a> {
    revision: u8,
    aces: Vec<&'a [u8]>,
}

impl<'a> Acl<'a> {
    /// Reads an ACL; `InvalidData` when a size or count runs past its bytes.
    pub(crate) fn parse(bytes: &'a [u8]) -> io::Result<Acl<'a>> {
        let short = || invalid("an ACL is shorter than it says");
        let revision = *bytes.first().ok_or_else(short)?;
        let size = usize::from(read_u16(bytes, 2).ok_or_else(short)?);
        let count = usize::from(read_u16(bytes, 4).ok_or_else(short)?);
        let body = bytes.get(..size).ok_or_else(short)?;
        let mut aces = Vec::with_capacity(count);
        let mut at = 8;
        for _ in 0..count {
            let len = usize::from(read_u16(body, at + 2).ok_or_else(short)?);
            if len < 4 {
                return Err(short());
            }
            aces.push(body.get(at..at + len).ok_or_else(short)?);
            at += len;
        }
        Ok(Acl { revision, aces })
    }

    /// Its entries, in order.
    pub(crate) fn aces(&self) -> &[&'a [u8]] {
        &self.aces
    }

    /// Whether `spec` is in it, explicitly.
    pub(crate) fn has(&self, spec: &AceSpec) -> bool {
        self.aces.iter().any(|ace| spec.is(ace))
    }

    /// This ACL with `spec` added where Windows' order puts it: explicit denies, explicit
    /// allows, then inherited entries in their order. A label replaces any explicit label.
    pub(crate) fn with(&self, spec: &AceSpec) -> Vec<u8> {
        let new = spec.bytes();
        let explicit = |a: &&[u8]| flags(a) & INHERITED == 0;
        let mut out: Vec<&[u8]> = Vec::with_capacity(self.aces.len() + 1);
        let keep = |a: &&&[u8]| !(spec.kind == LABEL && kind(a) == LABEL && explicit(a));
        let (ex, inherited): (Vec<&[u8]>, Vec<&[u8]>) =
            self.aces.iter().filter(keep).copied().partition(explicit);
        let (denies, others): (Vec<&[u8]>, Vec<&[u8]>) =
            ex.into_iter().partition(|a| is_deny_kind(kind(a)));
        out.extend(denies);
        if spec.kind != ALLOWED {
            out.push(&new);
        }
        out.extend(others);
        if spec.kind == ALLOWED {
            out.push(&new);
        }
        out.extend(inherited);
        render(self.revision.max(2), &out)
    }

    /// This ACL without `spec`; `None` when it isn't there.
    pub(crate) fn without(&self, spec: &AceSpec) -> Option<Vec<u8>> {
        let kept: Vec<&[u8]> = self
            .aces
            .iter()
            .copied()
            .filter(|ace| !spec.is(ace))
            .collect();
        (kept.len() != self.aces.len()).then(|| render(self.revision.max(2), &kept))
    }
}

/// An empty ACL's bytes.
#[cfg_attr(
    not(windows),
    expect(dead_code, reason = "the label writes are Windows only")
)]
pub(crate) fn empty_acl() -> Vec<u8> {
    render(2, &[])
}

/// Whether `dacl` holds an explicit allow, of any right but `SYNCHRONIZE`, that may beat an
/// inherited deny: a plain or callback allow for one of [`BROAD_SIDS`], of `watched` (the pool
/// group, the other pool users) or of the `None` group of a watched SID's machine
/// (`S-1-5-21-<machine>-513`: the Windows run found no pool token holding it, but it is kept as
/// broad, which costs nothing); or an allow of any other type
/// (object and compound allows, whose SID sits elsewhere), whoever it names. An explicit entry
/// is checked before every inherited one, so on a child it beats the inherited deny of a fence
/// or a sibling; an inherit-only one does the same for the child's children.
pub(crate) fn broad_allow(dacl: &Acl<'_>, watched: &[Vec<u8>]) -> bool {
    let mut broad: Vec<Vec<u8>> = BROAD_SIDS
        .iter()
        .filter_map(|s| Sid::parse(s).ok())
        .map(|s| sid_bytes(&s))
        .collect();
    for sid in watched.iter().filter_map(|w| sid_text(w)) {
        if let Some([a, b, c]) = sid.domain()
            && let Ok(none) = Sid::parse(&format!("S-1-5-21-{a}-{b}-{c}-{NONE_RID}"))
        {
            broad.push(sid_bytes(&none));
        }
    }
    dacl.aces().iter().any(|ace| {
        flags(ace) & INHERITED == 0
            && is_allow_kind(kind(ace))
            && mask(ace).is_some_and(|m| m & !SYNCHRONIZE != 0)
            && (!matches!(kind(ace), ALLOWED | ALLOWED_CALLBACK)
                || sid(ace).is_some_and(|s| broad.iter().chain(watched).any(|b| b == s)))
    })
}

/// The explicit mandatory label in `sacl`, if it has one: the first, as Windows reads it.
pub(crate) fn explicit_label(sacl: &Acl<'_>) -> Option<Label> {
    let ace = sacl
        .aces()
        .iter()
        .find(|ace| kind(ace) == LABEL && flags(ace) & INHERITED == 0)?;
    let level = sid(ace).and_then(sid_text)?;
    let level = level.as_str().strip_prefix("S-1-16-")?.parse().ok()?;
    Some(Label {
        level,
        flags: flags(ace),
        mask: mask(ace)?,
    })
}

/// The entry that writes `label`, explicitly.
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "the label writes are Windows only")
)]
pub(crate) fn label_spec(label: Label) -> AceSpec {
    let mut sid = vec![1, 1, 0, 0, 0, 0, 0, 16];
    sid.extend_from_slice(&label.level.to_le_bytes());
    AceSpec {
        kind: LABEL,
        flags: label.flags & !INHERITED,
        mask: label.mask,
        sid,
    }
}

/// The SACL a child gets from its parent's label, or `None` when its own needs no change: a
/// child with an explicit label, or a protected SACL, keeps it. Otherwise its label is the
/// parent's first label entry that reaches it (container inherit for a folder, object inherit
/// for a file), marked inherited, with the inheritance a folder passes on; no such entry, no
/// label. `child_sacl` holds the child's label entries only, as a label read returns them.
pub(crate) fn inherited_label(
    parent_sacl: Option<&Acl<'_>>,
    child_sacl: Option<&Acl<'_>>,
    child_control: u16,
    folder: bool,
) -> Option<Vec<u8>> {
    let labels = |acl: Option<&Acl<'_>>| -> Vec<Vec<u8>> {
        acl.map(|a| {
            a.aces()
                .iter()
                .filter(|ace| kind(ace) == LABEL)
                .map(|ace| ace.to_vec())
                .collect()
        })
        .unwrap_or_default()
    };
    let current = labels(child_sacl);
    if child_control & SACL_PROTECTED != 0 || current.iter().any(|a| flags(a) & INHERITED == 0) {
        return None;
    }
    let from = labels(parent_sacl).into_iter().find(|a| {
        let f = flags(a);
        if folder {
            f & (CONTAINER_INHERIT | OBJECT_INHERIT) != 0
                && (f & CONTAINER_INHERIT != 0 || f & NO_PROPAGATE == 0)
        } else {
            f & OBJECT_INHERIT != 0
        }
    });
    let want: Vec<Vec<u8>> = from
        .into_iter()
        .map(|mut ace| {
            let f = flags(&ace);
            let passed = if !folder || f & NO_PROPAGATE != 0 {
                0
            } else if f & CONTAINER_INHERIT != 0 {
                f & (CONTAINER_INHERIT | OBJECT_INHERIT)
            } else {
                OBJECT_INHERIT | INHERIT_ONLY
            };
            if let Some(slot) = ace.get_mut(1) {
                *slot = passed | INHERITED;
            }
            ace
        })
        .collect();
    if want == current {
        return None;
    }
    let refs: Vec<&[u8]> = want.iter().map(Vec::as_slice).collect();
    Some(render(2, &refs))
}

/// A self-relative security descriptor's parts, borrowed from its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Descriptor<'a> {
    pub(crate) control: u16,
    pub(crate) sacl: Option<&'a [u8]>,
    pub(crate) dacl: Option<&'a [u8]>,
}

impl<'a> Descriptor<'a> {
    /// Reads a self-relative descriptor; `InvalidData` when an offset runs past its bytes.
    pub(crate) fn parse(bytes: &'a [u8]) -> io::Result<Descriptor<'a>> {
        let control = read_u16(bytes, 2).ok_or_else(|| invalid("a short descriptor"))?;
        if control & SELF_RELATIVE == 0 {
            return Err(invalid("a descriptor that isn't self-relative"));
        }
        let acl_at = |offset_at: usize, present: u16| -> io::Result<Option<&'a [u8]>> {
            let offset = read_u32(bytes, offset_at).ok_or_else(|| invalid("a short descriptor"))?;
            if control & present == 0 || offset == 0 {
                return Ok(None);
            }
            let start = usize::try_from(offset).map_err(invalid)?;
            let rest = bytes
                .get(start..)
                .ok_or_else(|| invalid("an ACL past the end"))?;
            let size = usize::from(read_u16(rest, 2).ok_or_else(|| invalid("a short ACL"))?);
            rest.get(..size)
                .map(Some)
                .ok_or_else(|| invalid("an ACL past the end"))
        };
        Ok(Descriptor {
            control,
            sacl: acl_at(12, SACL_PRESENT)?,
            dacl: acl_at(16, DACL_PRESENT)?,
        })
    }

    /// Whether the DACL is protected from inheritance.
    pub(crate) fn dacl_protected(&self) -> bool {
        self.control & DACL_PROTECTED != 0
    }
}

/// A self-relative descriptor with no owner or group, `dacl` and `sacl` when given, and the
/// control bits `control` (the caller passes the inheritance bits to keep). A DACL marked
/// auto-inherited also asks to stay so (`SE_DACL_AUTO_INHERIT_REQ`): without it a write drops
/// the mark, every later walk rewrites the child, and the lawyer's folder loses its `AI` for
/// good (the Windows run at `fca0bf0ca`).
pub(crate) fn build_descriptor(control: u16, dacl: Option<&[u8]>, sacl: Option<&[u8]>) -> Vec<u8> {
    let mut control = control;
    if dacl.is_some() && control & DACL_AUTO_INHERITED != 0 {
        control |= DACL_AUTO_INHERIT_REQ;
    }
    assemble(control, [None, None, sacl, dacl])
}

/// `descriptor` with its DACL's inherited entries dropped and the DACL marked auto-inherited;
/// the owner, group, SACL and other control bits kept. A DACL not marked auto-inherited holds
/// copies of its parent's entries that `CreatePrivateObjectSecurityEx` keeps as explicit when it
/// works the child out again, so an inherited `Users` allow would become an explicit one and
/// refuse a strict walk (the Windows run 2, row 15, a plain `C:\m`). As
/// `SetNamedSecurityInfoW`'s propagation does, they go before the child is worked out from its
/// parent. A descriptor with no DACL comes back as it is.
#[cfg(any(windows, test))]
pub(crate) fn without_inherited(descriptor: &[u8]) -> io::Result<Vec<u8>> {
    let now = Descriptor::parse(descriptor)?;
    let Some(dacl) = now.dacl else {
        return Ok(descriptor.to_vec());
    };
    let dacl = Acl::parse(dacl)?;
    let kept: Vec<&[u8]> = dacl
        .aces()
        .iter()
        .copied()
        .filter(|ace| flags(ace) & INHERITED == 0)
        .collect();
    let dacl = render(dacl.revision.max(2), &kept);
    let sid_at = |offset_at: usize| -> io::Result<Option<&[u8]>> {
        let short = || invalid("a SID past the end");
        let offset = read_u32(descriptor, offset_at).ok_or_else(short)?;
        if offset == 0 {
            return Ok(None);
        }
        let rest = descriptor
            .get(usize::try_from(offset).map_err(invalid)?..)
            .ok_or_else(short)?;
        let count = usize::from(*rest.get(1).ok_or_else(short)?);
        rest.get(..8 + 4 * count).map(Some).ok_or_else(short)
    };
    let parts = [sid_at(4)?, sid_at(8)?, now.sacl, Some(dacl.as_slice())];
    Ok(assemble(now.control | DACL_AUTO_INHERITED, parts))
}

/// A self-relative descriptor of `control` and `parts` (owner, group, SACL, DACL), each
/// present one put after the header with its offset, and the SACL's and DACL's present bits set.
fn assemble(control: u16, parts: [Option<&[u8]>; 4]) -> Vec<u8> {
    let [owner, group, sacl, dacl] = parts;
    let mut control = control | SELF_RELATIVE;
    let mut out = vec![1, 0, 0, 0];
    out.extend_from_slice(&[0; 16]);
    let mut put = |acl: Option<&[u8]>, offset_at: usize, present: u16, out: &mut Vec<u8>| {
        if let Some(acl) = acl {
            control |= present;
            let offset = u32::try_from(out.len()).unwrap_or(0).to_le_bytes();
            if let Some(slot) = out.get_mut(offset_at..offset_at + 4) {
                slot.copy_from_slice(&offset);
            }
            out.extend_from_slice(acl);
        }
    };
    put(owner, 4, 0, &mut out);
    put(group, 8, 0, &mut out);
    put(sacl, 12, SACL_PRESENT, &mut out);
    put(dacl, 16, DACL_PRESENT, &mut out);
    if let Some(slot) = out.get_mut(2..4) {
        slot.copy_from_slice(&control.to_le_bytes());
    }
    out
}

/// An ACL of `aces` at `revision`.
fn render(revision: u8, aces: &[&[u8]]) -> Vec<u8> {
    let size = 8 + aces.iter().map(|a| a.len()).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&[revision, 0]);
    out.extend_from_slice(&u16::try_from(size).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&u16::try_from(aces.len()).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for ace in aces {
        out.extend_from_slice(ace);
    }
    out
}

/// Allow types: plain, compound, object, callback and callback object.
fn is_allow_kind(kind: u8) -> bool {
    matches!(kind, 0 | 4 | 5 | 9 | 11)
}

/// Deny types: plain, object, callback and callback object.
fn is_deny_kind(kind: u8) -> bool {
    matches!(kind, 1 | 6 | 10 | 12)
}

/// An entry's type.
pub(crate) fn kind(ace: &[u8]) -> u8 {
    ace.first().copied().unwrap_or(u8::MAX)
}

/// An entry's flags.
pub(crate) fn flags(ace: &[u8]) -> u8 {
    ace.get(1).copied().unwrap_or(0)
}

/// An entry's mask, for the types that put it first (all but object entries' extra fields).
pub(crate) fn mask(ace: &[u8]) -> Option<u32> {
    read_u32(ace, 4)
}

/// An entry's SID, for the plain, callback and label types (the SID follows the mask).
pub(crate) fn sid(ace: &[u8]) -> Option<&[u8]> {
    if !matches!(kind(ace), 0 | 1 | 9 | 10 | LABEL) {
        return None;
    }
    let body = ace.get(8..)?;
    let count = usize::from(*body.get(1)?);
    body.get(..8 + 4 * count)
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests;
