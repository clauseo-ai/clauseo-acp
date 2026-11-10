//! What the papers scan does with a Windows file it opened as a reparse point itself
//! (`FILE_FLAG_OPEN_REPARSE_POINT`), by the attributes and reparse tag the handle reports. A link
//! is never followed; a cloud placeholder or an offline file is never read, since reading it
//! would download it (a scan must not fetch a whole matter kept in the cloud); any other
//! reparse point, such as a deduplicated file, opened as itself reads as zeros, so it is opened
//! again normally.
//! Plain numbers, so the decision is tested on every OS.

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
const REPARSE_POINT: u32 = 0x400;
/// `FILE_ATTRIBUTE_OFFLINE`: the data is not on this computer.
const OFFLINE: u32 = 0x1000;
/// `FILE_ATTRIBUTE_RECALL_ON_OPEN`: opening the file fetches it.
const RECALL_ON_OPEN: u32 = 0x4_0000;
/// `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`: reading the file fetches it.
const RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// `IO_REPARSE_TAG_MOUNT_POINT`: a junction or a mounted volume.
const TAG_MOUNT_POINT: u32 = 0xA000_0003;
/// `IO_REPARSE_TAG_SYMLINK`.
const TAG_SYMLINK: u32 = 0xA000_000C;
/// `IO_REPARSE_TAG_CLOUD`, and with any of [`TAG_CLOUD_MASK`] set, `IO_REPARSE_TAG_CLOUD_1` to
/// `_F`: a cloud files placeholder (Microsoft's sync client and others on the cloud files API).
const TAG_CLOUD: u32 = 0x9000_001A;
/// `IO_REPARSE_TAG_CLOUD_MASK`.
const TAG_CLOUD_MASK: u32 = 0x0000_F000;
/// `IO_REPARSE_TAG_ONEDRIVE`: the older placeholders of Microsoft's sync client.
const TAG_ONEDRIVE: u32 = 0x8000_0021;
/// `IO_REPARSE_TAG_HSM` and `IO_REPARSE_TAG_HSM2`: hierarchical storage, the data elsewhere.
const TAG_HSM: [u32; 2] = [0xC000_0004, 0x8000_0006];

/// What to do with the opened file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reparse {
    /// Not a reparse point: read the handle as it is.
    Read,
    /// Another reparse point (deduplicated, compressed by WOF…): open it again without the flag
    /// and read that.
    Reopen,
    /// A link or a mount point: never followed, so not read.
    Skip,
    /// In the cloud or offline: unknown, never read and never kept.
    Unknown,
}

/// What to do with a file whose handle reports `attributes` and `tag`.
pub(super) fn reparse(attributes: u32, tag: u32) -> Reparse {
    if attributes & (OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS) != 0 {
        return Reparse::Unknown;
    }
    if attributes & REPARSE_POINT == 0 {
        return Reparse::Read;
    }
    match tag {
        TAG_SYMLINK | TAG_MOUNT_POINT => Reparse::Skip,
        TAG_ONEDRIVE => Reparse::Unknown,
        tag if tag & !TAG_CLOUD_MASK == TAG_CLOUD || TAG_HSM.contains(&tag) => Reparse::Unknown,
        _ => Reparse::Reopen,
    }
}

#[cfg(test)]
mod tests {
    use super::{REPARSE_POINT, Reparse, reparse};

    /// `FILE_ATTRIBUTE_ARCHIVE`, as most files carry.
    const ARCHIVE: u32 = 0x20;

    #[test]
    fn a_plain_file_is_read_and_a_link_or_mount_point_skipped() {
        assert_eq!(reparse(ARCHIVE, 0), Reparse::Read);
        // the tag means nothing without the attribute
        assert_eq!(reparse(ARCHIVE, 0xA000_000C), Reparse::Read);
        assert_eq!(reparse(ARCHIVE | REPARSE_POINT, 0xA000_000C), Reparse::Skip);
        assert_eq!(reparse(REPARSE_POINT, 0xA000_0003), Reparse::Skip);
    }

    #[test]
    fn a_cloud_placeholder_or_an_offline_file_is_unknown() {
        // IO_REPARSE_TAG_CLOUD, _CLOUD_3 and _CLOUD_F, as the sync client sets them
        for tag in [0x9000_001A, 0x9000_301A, 0x9000_F01A] {
            assert_eq!(
                reparse(ARCHIVE | REPARSE_POINT, tag),
                Reparse::Unknown,
                "{tag:#x}"
            );
        }
        assert_eq!(reparse(REPARSE_POINT, 0x8000_0021), Reparse::Unknown);
        assert_eq!(reparse(REPARSE_POINT, 0xC000_0004), Reparse::Unknown);
        // offline or recalled on access, whatever the tag, even with none
        assert_eq!(reparse(ARCHIVE | 0x1000, 0), Reparse::Unknown);
        assert_eq!(reparse(ARCHIVE | 0x4_0000, 0), Reparse::Unknown);
        assert_eq!(
            reparse(REPARSE_POINT | 0x40_0000, 0x8000_0013),
            Reparse::Unknown
        );
        assert_eq!(
            reparse(REPARSE_POINT | 0x40_0000, 0xA000_000C),
            Reparse::Unknown
        );
    }

    #[test]
    fn any_other_reparse_point_is_opened_again() {
        // IO_REPARSE_TAG_DEDUP, IO_REPARSE_TAG_WOF, and a tag nobody knows
        for tag in [0x8000_0013, 0x8000_0017, 0x0000_1234] {
            assert_eq!(
                reparse(ARCHIVE | REPARSE_POINT, tag),
                Reparse::Reopen,
                "{tag:#x}"
            );
        }
    }
}
