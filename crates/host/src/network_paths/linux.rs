//! Linux: the mount points that may live on another machine, from `/proc/self/mounts`, read as
//! bytes. An allow list: a mount counts as local only when its type is a known local one
//! ([`LOCAL_TYPES`]); every other type is remote, so a network or FUSE file system nobody listed
//! (`autofs`, `fuse.gvfsd-fuse`, `fuse.rclone`, `9p`, `virtiofs`, plain `fuse`) is refused.

use std::path::PathBuf;

/// The file system types that live on this machine: disk file systems, memory and image file
/// systems, and the kernel's own. Every other type counts as remote.
pub(crate) const LOCAL_TYPES: [&str; 46] = [
    "ext2",
    "ext3",
    "ext4",
    "xfs",
    "btrfs",
    "zfs",
    "bcachefs",
    "f2fs",
    "jfs",
    "reiserfs",
    "nilfs2",
    "vfat",
    "msdos",
    "exfat",
    "ntfs",
    "ntfs3",
    "hfsplus",
    "iso9660",
    "udf",
    "fuseblk",
    "tmpfs",
    "ramfs",
    "rootfs",
    "overlay",
    "squashfs",
    "erofs",
    "devtmpfs",
    "devpts",
    "proc",
    "sysfs",
    "cgroup",
    "cgroup2",
    "mqueue",
    "hugetlbfs",
    "securityfs",
    "debugfs",
    "tracefs",
    "pstore",
    "bpf",
    "configfs",
    "fusectl",
    "binfmt_misc",
    "efivarfs",
    "nsfs",
    "selinuxfs",
    "fuse.portal",
];

/// The mount points in a Linux mount table (`/proc/self/mounts`) whose type is not in
/// [`LOCAL_TYPES`]: `device mountpoint type options 0 0`, the mount point's space, tab, newline
/// and `\` written as octal escapes, its other bytes as they are (not always UTF-8). `None` when
/// a line does not parse: the table is then unknown, and the caller counts every path as remote.
pub(crate) fn remote_mounts(table: &[u8]) -> Option<Vec<PathBuf>> {
    let mut mounts = Vec::new();
    for line in table.split(|&b| b == b'\n') {
        let mut fields = line
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty());
        let Some(device) = fields.next() else {
            continue;
        };
        let (_, Some(mount), Some(kind)) = (device, fields.next(), fields.next()) else {
            return None;
        };
        if !LOCAL_TYPES.iter().any(|t| t.as_bytes() == kind) {
            mounts.push(path_of_bytes(unescape_octal(mount)));
        }
    }
    Some(mounts)
}

/// A path from its bytes: as they are on Unix, read as UTF-8 (lossily) elsewhere.
fn path_of_bytes(bytes: Vec<u8>) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt as _;
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(
            String::from_utf8(bytes)
                .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()),
        )
    }
}

/// `\040`-style escapes made the bytes they stand for.
fn unescape_octal(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        let digits = text.get(at.saturating_add(1)..at.saturating_add(4));
        let code = digits
            .filter(|_| byte == b'\\')
            .filter(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)))
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        if let Some(code) = code {
            out.push(code);
            at = at.saturating_add(4);
        } else {
            out.push(byte);
            at = at.saturating_add(1);
        }
    }
    out
}
