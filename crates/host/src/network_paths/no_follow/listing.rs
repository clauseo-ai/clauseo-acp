//! The pure halves of Windows' no-follow open, read from the bytes a call filled in, so they are
//! tested on every system: one entry of a folder's listing (`FILE_FULL_DIR_INFORMATION`, its
//! attributes and reparse tag) and a link's target (`REPARSE_DATA_BUFFER`, as `std`'s
//! `read_link` reads it).

/// `FILE_ATTRIBUTE_DIRECTORY`.
pub(crate) const DIRECTORY: u32 = 0x10;
/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub(crate) const REPARSE_POINT: u32 = 0x400;
/// `IO_REPARSE_TAG_MOUNT_POINT` (a junction).
const MOUNT_POINT: u32 = 0xA000_0003;
/// `IO_REPARSE_TAG_SYMLINK`.
const SYMLINK: u32 = 0xA000_000C;
/// `SYMLINK_FLAG_RELATIVE`.
const RELATIVE: u32 = 0x1;

/// One listing entry: its attributes, its reparse tag (0 for none) and its name (UTF-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryInfo {
    pub(crate) attributes: u32,
    pub(crate) tag: u32,
    pub(crate) name: Vec<u16>,
}

/// Whether a reparse tag is a name surrogate (bit 29: it names another file, a symlink or a
/// junction): `std` reads such a reparse point as a link, and so does the walk.
pub(crate) fn name_surrogate(tag: u32) -> bool {
    tag & 0x2000_0000 != 0
}

/// The `u32` at `at` in `bytes`, little-endian.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// The `u16` at `at` in `bytes`, little-endian.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// `len` bytes at `at` in `bytes` read as UTF-16 units.
fn wide_at(bytes: &[u8], at: usize, len: usize) -> Option<Vec<u16>> {
    let raw = bytes.get(at..at.checked_add(len)?)?;
    if raw.len() % 2 != 0 {
        return None;
    }
    Some(
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect(),
    )
}

/// The first `FILE_FULL_DIR_INFORMATION` in `bytes` and its `NextEntryOffset` (0 for the last):
/// `FileAttributes` at 56, `FileNameLength` at 60, `EaSize` at 64 (the reparse tag when the entry
/// is a reparse point, as `FindFirstFileW`'s `dwReserved0`), `FileName` from 68. `None` when the
/// bytes are too short.
pub(crate) fn dir_entry(bytes: &[u8]) -> Option<(EntryInfo, usize)> {
    let next = usize::try_from(u32_at(bytes, 0)?).ok()?;
    let attributes = u32_at(bytes, 56)?;
    let name_len = usize::try_from(u32_at(bytes, 60)?).ok()?;
    let ea_or_tag = u32_at(bytes, 64)?;
    let name = wide_at(bytes, 68, name_len)?;
    let tag = if attributes & REPARSE_POINT == 0 {
        0
    } else {
        ea_or_tag
    };
    Some((
        EntryInfo {
            attributes,
            tag,
            name,
        },
        next,
    ))
}

/// The target a link's `REPARSE_DATA_BUFFER` names, as `std`'s `read_link` gives it: the
/// substitute name, an absolute one's NT prefix `\??\` turned into `\\?\`. A symlink's names
/// start at 20 (after its flags), a junction's at 16. `None` for any other tag or short bytes.
pub(crate) fn link_target(bytes: &[u8]) -> Option<Vec<u16>> {
    let tag = u32_at(bytes, 0)?;
    let (names_at, relative): (usize, bool) = match tag {
        SYMLINK => (20, u32_at(bytes, 16)? & RELATIVE != 0),
        MOUNT_POINT => (16, false),
        _ => return None,
    };
    let offset = usize::from(u16_at(bytes, 8)?);
    let len = usize::from(u16_at(bytes, 10)?);
    let mut target = wide_at(bytes, names_at.checked_add(offset)?, len)?;
    let nt_prefix = (*b"\\??\\").map(u16::from);
    if !relative && target.starts_with(&nt_prefix) {
        // `\??\` → `\\?\`.
        if let Some(second) = target.get_mut(1) {
            *second = u16::from(b'\\');
        }
    }
    Some(target)
}

#[cfg(test)]
mod tests {
    use super::{DIRECTORY, EntryInfo, REPARSE_POINT, dir_entry, link_target, name_surrogate};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn le(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    /// A `FILE_FULL_DIR_INFORMATION` with `attributes`, `ea_or_tag` and `name`.
    fn entry(next: u32, attributes: u32, ea_or_tag: u32, name: &str) -> Vec<u8> {
        let name = le(&wide(name));
        let mut bytes = vec![0_u8; 68];
        bytes[0..4].copy_from_slice(&next.to_le_bytes());
        bytes[56..60].copy_from_slice(&attributes.to_le_bytes());
        bytes[60..64].copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        bytes[64..68].copy_from_slice(&ea_or_tag.to_le_bytes());
        bytes.extend(name);
        bytes
    }

    #[test]
    fn a_listing_entry_gives_its_attributes_tag_and_name() {
        let (info, next) =
            dir_entry(&entry(96, DIRECTORY | REPARSE_POINT, 0xA000_0003, "j")).unwrap();
        assert_eq!(
            info,
            EntryInfo {
                attributes: DIRECTORY | REPARSE_POINT,
                tag: 0xA000_0003,
                name: wide("j"),
            }
        );
        assert_eq!(next, 96);
        // Not a reparse point: EaSize is an EA size, never a tag.
        let (info, _) = dir_entry(&entry(0, 0x20, 0xA000_000C, "Brief.docx")).unwrap();
        assert_eq!(info.tag, 0);
        assert_eq!(info.name, wide("Brief.docx"));
        let mut short = entry(0, 0x20, 0, "Brief.docx");
        short.truncate(70);
        assert_eq!(dir_entry(&short), None);
    }

    /// A reparse buffer of `tag` with the substitute name `subst` and print name `print`.
    fn reparse(tag: u32, flags: Option<u32>, subst: &str, print: &str) -> Vec<u8> {
        let (subst, print) = (le(&wide(subst)), le(&wide(print)));
        let mut bytes = Vec::new();
        bytes.extend(tag.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        let len = |b: &Vec<u8>| u16::try_from(b.len()).unwrap();
        // Print name first in the buffer, the substitute name after it.
        bytes.extend(len(&print).to_le_bytes());
        bytes.extend(len(&subst).to_le_bytes());
        bytes.extend(0_u16.to_le_bytes());
        bytes.extend(len(&print).to_le_bytes());
        if let Some(flags) = flags {
            bytes.extend(flags.to_le_bytes());
        }
        bytes.extend(print);
        bytes.extend(subst);
        bytes
    }

    #[test]
    fn a_link_target_is_its_substitute_name_with_the_nt_prefix_made_verbatim() {
        let text = |units: Vec<u16>| String::from_utf16(&units).unwrap();
        let junction = reparse(0xA000_0003, None, r"\??\C:\Users\x", r"C:\Users\x");
        assert_eq!(text(link_target(&junction).unwrap()), r"\\?\C:\Users\x");
        let unc = reparse(0xA000_000C, Some(0), r"\??\UNC\srv\share", r"\\srv\share");
        assert_eq!(text(link_target(&unc).unwrap()), r"\\?\UNC\srv\share");
        let relative = reparse(0xA000_000C, Some(1), r"..\deep", r"..\deep");
        assert_eq!(text(link_target(&relative).unwrap()), r"..\deep");
        assert_eq!(link_target(&reparse(0x8000_0013, None, "x", "x")), None);
        assert_eq!(link_target(&junction[..12]), None);
    }

    #[test]
    fn links_are_the_name_surrogate_tags() {
        assert!(name_surrogate(0xA000_0003));
        assert!(name_surrogate(0xA000_000C));
        assert!(!name_surrogate(0x9000_001A));
        assert!(!name_surrogate(0x8000_0013));
    }
}
