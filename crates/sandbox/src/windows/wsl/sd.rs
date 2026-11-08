//! A COM launch permission (`LaunchPermission`, `DefaultLaunchPermission`) as bytes: a
//! self-relative security descriptor read strictly, edited for one group's deny, and written
//! back with its parts in the order it had. Pure; tested on every OS.
//!
//! Layout, little-endian: `SECURITY_DESCRIPTOR_RELATIVE` (revision 1, a pad byte, the control
//! word, then the offsets of the owner, group, SACL and DACL), each `ACL` (revision, size, count,
//! then its entries), each entry (type, flags, size, mask, then for the plain types the SID).
//! COM requires an owner and a group, so a descriptor without either is refused, as is one with
//! no DACL or a NULL one (which would grant everyone). Whatever is refused is never written.

use crate::windows::sids::Sid;

/// The deny's rights: `CCDCLCSWRP`, `COM_RIGHTS_EXECUTE` and the local and remote launch and
/// activation rights.
pub const DENY_MASK: u32 = 0x1F;
/// The rights a deny already there must hold to count: `COM_RIGHTS_EXECUTE`,
/// `COM_RIGHTS_EXECUTE_LOCAL` and `COM_RIGHTS_ACTIVATE_LOCAL`, the ones wslservice's own
/// descriptor grants (`0xB`).
pub const NEEDED_MASK: u32 = 0xB;

/// `SE_SELF_RELATIVE`.
const SELF_RELATIVE: u16 = 0x8000;
/// `SE_DACL_PRESENT`.
const DACL_PRESENT: u16 = 0x0004;
/// `SE_SACL_PRESENT`.
const SACL_PRESENT: u16 = 0x0010;
/// The header's size.
const HEADER: usize = 20;
/// `ACCESS_ALLOWED_ACE_TYPE`.
const ALLOWED: u8 = 0;
/// `ACCESS_DENIED_ACE_TYPE`.
const DENIED: u8 = 1;
/// The most sub-authorities a SID has.
const MAX_SUBS: usize = 15;
/// `BUILTIN\Administrators`, `INTERACTIVE` and `SYSTEM`: COM's built-in launch default.
const BUILT_IN: [&str; 3] = ["S-1-5-32-544", "S-1-5-4", "S-1-5-18"];

/// Why a descriptor was refused. Names no path or SID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the launch permission is malformed: {0}")]
pub struct SdError(pub &'static str);

/// One part of a descriptor, for the order its parts are written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Owner,
    Group,
    Sacl,
    Dacl,
}

/// A launch permission, parsed.
#[derive(Debug, Clone)]
pub struct LaunchSd {
    /// The control word, as read.
    control: u16,
    /// The owner SID's bytes.
    owner: Vec<u8>,
    /// The group SID's bytes.
    group: Vec<u8>,
    /// The SACL's bytes, as read; `None` when there is none (or a NULL one).
    sacl: Option<Vec<u8>>,
    /// The DACL's revision.
    revision: u8,
    /// The DACL's entries, each whole, in order.
    aces: Vec<Vec<u8>>,
    /// The parts in the order of their offsets, as read.
    order: Vec<Part>,
}

/// Two descriptors are the same when they grant and deny the same, whatever order their parts
/// are laid out in.
impl PartialEq for LaunchSd {
    fn eq(&self, other: &Self) -> bool {
        self.control == other.control
            && self.owner == other.owner
            && self.group == other.group
            && self.sacl == other.sacl
            && self.revision == other.revision
            && self.aces == other.aces
    }
}

impl Eq for LaunchSd {}

impl LaunchSd {
    /// Reads `bytes` as the module says; anything odd is an [`SdError`].
    pub fn parse(bytes: &[u8]) -> Result<LaunchSd, SdError> {
        if bytes.len() < HEADER {
            return Err(SdError("shorter than its header"));
        }
        if bytes.first() != Some(&1) {
            return Err(SdError("not revision 1"));
        }
        let control = u16_at(bytes, 2).ok_or(SdError("no control word"))?;
        if control & SELF_RELATIVE == 0 {
            return Err(SdError("not self-relative"));
        }
        let offset = |at: usize| -> Result<usize, SdError> {
            let raw = u32_at(bytes, at).ok_or(SdError("no offset"))?;
            let offset =
                usize::try_from(raw).map_err(|_too_far| SdError("an offset past the end"))?;
            if offset != 0 && offset < HEADER {
                return Err(SdError("an offset inside the header"));
            }
            Ok(offset)
        };
        let (owner_at, group_at, sacl_at, dacl_at) =
            (offset(4)?, offset(8)?, offset(12)?, offset(16)?);
        if owner_at == 0 || group_at == 0 {
            return Err(SdError("no owner or no group"));
        }
        if control & DACL_PRESENT == 0 || dacl_at == 0 {
            return Err(SdError("no DACL, or a NULL one"));
        }
        let owner = sid_at(bytes, owner_at)?.to_vec();
        let group = sid_at(bytes, group_at)?.to_vec();
        let (revision, aces) =
            acl_at(bytes, dacl_at).map(|(revision, aces, _)| (revision, aces))?;
        let sacl = if control & SACL_PRESENT != 0 && sacl_at != 0 {
            Some(acl_at(bytes, sacl_at)?.2.to_vec())
        } else {
            None
        };
        let mut placed = vec![
            (owner_at, Part::Owner),
            (group_at, Part::Group),
            (dacl_at, Part::Dacl),
        ];
        if sacl.is_some() {
            placed.push((sacl_at, Part::Sacl));
        }
        placed.sort_by_key(|(at, _)| *at);
        Ok(LaunchSd {
            control,
            owner,
            group,
            sacl,
            revision,
            aces,
            order: placed.into_iter().map(|(_, part)| part).collect(),
        })
    }

    /// COM's built-in launch permission, for an `AppID` with no value when the machine has no
    /// `DefaultLaunchPermission` either: `O:BAG:BAD:(A;;CCDCLCSWRP;;;BA)(A;;CCDCLCSWRP;;;IU)
    /// (A;;CCDCLCSWRP;;;SY)`, as the VM's `DefaultLaunchPermission` reads.
    pub fn built_in_default() -> LaunchSd {
        // S-1-5-32-544.
        let administrators = vec![1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x20, 2, 0, 0];
        let aces = BUILT_IN
            .iter()
            .filter_map(|text| Sid::parse(text).ok())
            .map(|sid| ace(ALLOWED, DENY_MASK, &sid_bytes(&sid)))
            .collect();
        LaunchSd {
            control: SELF_RELATIVE | DACL_PRESENT,
            owner: administrators.clone(),
            group: administrators,
            sacl: None,
            revision: 2,
            aces,
            order: vec![Part::Owner, Part::Group, Part::Dacl],
        }
    }

    /// The descriptor as self-relative bytes, its parts in the order they were read.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = vec![1, 0];
        let mut control = self.control | SELF_RELATIVE | DACL_PRESENT;
        if self.sacl.is_some() {
            control |= SACL_PRESENT;
        }
        out.extend_from_slice(&control.to_le_bytes());
        out.extend_from_slice(&[0; 16]);
        let dacl = render(self.revision, &self.aces);
        for part in &self.order {
            let (bytes, offset_at) = match part {
                Part::Owner => (self.owner.as_slice(), 4),
                Part::Group => (self.group.as_slice(), 8),
                Part::Sacl => match &self.sacl {
                    Some(sacl) => (sacl.as_slice(), 12),
                    None => continue,
                },
                Part::Dacl => (dacl.as_slice(), 16),
            };
            let offset = u32::try_from(out.len()).unwrap_or(u32::MAX).to_le_bytes();
            if let Some(slot) = out.get_mut(offset_at..offset_at + 4) {
                slot.copy_from_slice(&offset);
            }
            out.extend_from_slice(bytes);
        }
        out
    }

    /// Whether a deny for `group`, holding at least [`NEEDED_MASK`] and applying to the object
    /// itself, comes before every allow entry.
    pub fn denies_first(&self, group: &Sid) -> bool {
        let group = sid_bytes(group);
        for entry in &self.aces {
            if is_allow(kind(entry)) {
                return false;
            }
            let applies = flags(entry) & INHERIT_ONLY == 0;
            let strong = mask(entry).is_some_and(|mask| mask & NEEDED_MASK == NEEDED_MASK);
            if kind(entry) == DENIED && applies && strong && sid(entry) == Some(group.as_slice()) {
                return true;
            }
        }
        false
    }

    /// Whether any deny entry names `group`.
    pub fn names_deny(&self, group: &Sid) -> bool {
        let group = sid_bytes(group);
        self.aces.iter().any(|entry| is_our_deny(entry, &group))
    }

    /// This descriptor with `(D;;CCDCLCSWRP;;;group)` at DACL index 0 and everything else kept;
    /// `None` when [`LaunchSd::denies_first`] holds already.
    pub fn with_deny(&self, group: &Sid) -> Option<LaunchSd> {
        if self.denies_first(group) {
            return None;
        }
        let mut merged = self.clone();
        merged
            .aces
            .insert(0, ace(DENIED, DENY_MASK, &sid_bytes(group)));
        Some(merged)
    }

    /// This descriptor without any deny entry for `group` (`(D;...;;;group)`, whatever its
    /// flags and mask), and how many went.
    pub fn without_denies(&self, group: &Sid) -> (LaunchSd, usize) {
        let group = sid_bytes(group);
        let mut kept = self.clone();
        kept.aces.retain(|entry| !is_our_deny(entry, &group));
        let dropped = self.aces.len() - kept.aces.len();
        (kept, dropped)
    }
}

/// `INHERIT_ONLY_ACE`.
const INHERIT_ONLY: u8 = 0x8;

/// Whether `entry` is a plain deny naming `group`'s bytes.
fn is_our_deny(entry: &[u8], group: &[u8]) -> bool {
    kind(entry) == DENIED && sid(entry) == Some(group)
}

/// Allow types: plain, compound, object, callback and callback object.
fn is_allow(kind: u8) -> bool {
    matches!(kind, 0 | 4 | 5 | 9 | 11)
}

/// Whether entries of type `kind` hold a SID right after their mask: plain allow, deny, audit
/// and alarm, their callback forms, and the label.
fn has_sid(kind: u8) -> bool {
    matches!(kind, 0..=3 | 9 | 10 | 13 | 0x11)
}

/// An entry of `kind` with no flags, `mask` and the SID `sid`.
fn ace(kind: u8, mask: u32, sid: &[u8]) -> Vec<u8> {
    let size = u16::try_from(8 + sid.len()).unwrap_or(u16::MAX);
    let mut out = vec![kind, 0];
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(sid);
    out
}

/// An ACL of `aces` at `revision`.
fn render(revision: u8, aces: &[Vec<u8>]) -> Vec<u8> {
    let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
    let mut out = vec![revision, 0];
    out.extend_from_slice(&u16::try_from(size).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&u16::try_from(aces.len()).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    for entry in aces {
        out.extend_from_slice(entry);
    }
    out
}

/// The SID at `offset` in `bytes`: revision 1, at most [`MAX_SUBS`] sub-authorities, whole.
fn sid_at(bytes: &[u8], offset: usize) -> Result<&[u8], SdError> {
    let rest = bytes.get(offset..).ok_or(SdError("a SID past the end"))?;
    whole_sid(rest).ok_or(SdError("a SID that runs past the end or isn't one"))
}

/// The SID that starts `bytes`, when it is a whole one.
fn whole_sid(bytes: &[u8]) -> Option<&[u8]> {
    let count = usize::from(*bytes.get(1)?);
    if bytes.first() != Some(&1) || count > MAX_SUBS {
        return None;
    }
    bytes.get(..8 + 4 * count)
}

/// An ACL's revision, its entries, and its bytes.
type AclParts<'a> = (u8, Vec<Vec<u8>>, &'a [u8]);

/// The ACL at `offset` in `bytes`: its revision, its entries, and its bytes. Revision 2 or 4,
/// a size of at least its header, within `bytes` and a multiple of 4, and entries that fill
/// no more than it, each a multiple of 4 and, for the plain types, holding a whole SID.
fn acl_at(bytes: &[u8], offset: usize) -> Result<AclParts<'_>, SdError> {
    let rest = bytes.get(offset..).ok_or(SdError("an ACL past the end"))?;
    let revision = *rest.first().ok_or(SdError("an ACL past the end"))?;
    if !matches!(revision, 2 | 4) {
        return Err(SdError("an ACL of an unknown revision"));
    }
    let size = usize::from(u16_at(rest, 2).ok_or(SdError("a short ACL"))?);
    let count = usize::from(u16_at(rest, 4).ok_or(SdError("a short ACL"))?);
    if size < 8 || size % 4 != 0 {
        return Err(SdError("an ACL of an odd size"));
    }
    let body = rest.get(..size).ok_or(SdError("an ACL past the end"))?;
    let mut aces = Vec::with_capacity(count);
    let mut at = 8;
    for _ in 0..count {
        let len = usize::from(u16_at(body, at + 2).ok_or(SdError("an entry past its ACL"))?);
        if len < 8 || len % 4 != 0 {
            return Err(SdError("an entry of an odd size"));
        }
        let entry = body
            .get(at..at + len)
            .ok_or(SdError("an entry past its ACL"))?;
        if has_sid(kind(entry)) && entry.get(8..).and_then(whole_sid).is_none() {
            return Err(SdError("an entry whose SID isn't whole"));
        }
        aces.push(entry.to_vec());
        at += len;
    }
    Ok((revision, aces, body))
}

/// `sid`'s bytes: revision 1, the count, the authority (six bytes, big-endian), then each
/// sub-authority (four bytes, little-endian).
pub(super) fn sid_bytes(sid: &Sid) -> Vec<u8> {
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

/// An entry's type.
fn kind(entry: &[u8]) -> u8 {
    entry.first().copied().unwrap_or(u8::MAX)
}

/// An entry's flags.
fn flags(entry: &[u8]) -> u8 {
    entry.get(1).copied().unwrap_or(0)
}

/// An entry's mask.
fn mask(entry: &[u8]) -> Option<u32> {
    u32_at(entry, 4)
}

/// A plain entry's SID.
fn sid(entry: &[u8]) -> Option<&[u8]> {
    if !has_sid(kind(entry)) {
        return None;
    }
    entry.get(8..).and_then(whole_sid)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests;
