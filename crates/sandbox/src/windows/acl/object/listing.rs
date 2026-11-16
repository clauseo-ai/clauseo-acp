//! A folder's listing read (S15 PR3, beside `object.rs`): the names, kinds and file IDs in a
//! `FileIdBothDirectoryInfo` chain, the `.` and `..` entries left out.

use std::io;

use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO,
};

use super::Child;

/// Adds the names in one `FILE_ID_BOTH_DIR_INFO` chain, `bytes`, to `out`.
pub(super) fn listed(bytes: &[u8], out: &mut Vec<Child>) -> io::Result<()> {
    let attributes_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes);
    let length_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
    let name_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let id_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileId);
    let short = || io::Error::new(io::ErrorKind::InvalidData, "a short folder listing");
    let mut at = 0_usize;
    loop {
        let entry = bytes.get(at..).ok_or_else(short)?;
        let next = read_u32(entry, 0).ok_or_else(short)?;
        let attributes = read_u32(entry, attributes_at).ok_or_else(short)?;
        let length = usize::try_from(read_u32(entry, length_at).ok_or_else(short)?)
            .map_err(io::Error::other)?;
        let id = read_u64(entry, id_at).ok_or_else(short)?;
        let raw = entry.get(name_at..name_at + length).ok_or_else(short)?;
        let name: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let dots = name == [u16::from(b'.')] || name == [u16::from(b'.'), u16::from(b'.')];
        if !dots && !name.is_empty() {
            out.push(Child {
                name,
                folder: attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
                reparse: attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
                id,
            });
        }
        if next == 0 {
            return Ok(());
        }
        at += usize::try_from(next).map_err(io::Error::other)?;
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
