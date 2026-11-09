//! Network paths the walk never touches. A `gitdir:` line or a `commondir` file can name a
//! server's share (`\\host\share`, a mapped drive, an SMB or NFS mount); resolving it at launch
//! would connect to that server, which can send the user's NTLM hash or hang the launch. So the
//! walk classifies a named path by its text and the mount table first, and skips a network one
//! before any file call.

use std::path::{Component, Path, PathBuf};

use crate::policy::under;

/// The mount table, read without touching any server: macOS's mount list and Windows's remote
/// drive letters. Elsewhere it is empty, and only a path's text marks it as a network path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Mounts {
    /// Mount points, each with whether its file system is a network one.
    points: Vec<(PathBuf, bool)>,
    /// The upper-case letters of `DRIVE_REMOTE` drives.
    remote_drives: Vec<u8>,
}

/// The macOS file system types that reach a server through the mount.
#[cfg(any(target_os = "macos", test))]
const NETWORK_FS: [&str; 6] = ["smbfs", "nfs", "afpfs", "webdav", "cifs", "autofs"];

impl Mounts {
    /// A table of these mount points (with whether each is a network one) and remote drives.
    #[cfg(test)]
    pub(crate) fn new(points: Vec<(PathBuf, bool)>, remote_drives: &[u8]) -> Mounts {
        Mounts {
            points,
            remote_drives: remote_drives.iter().map(u8::to_ascii_uppercase).collect(),
        }
    }

    /// Whether the file system type `kind` (a mount's `f_fstypename`) is a network one.
    /// `autofs` counts: looking under one makes it mount its server.
    #[cfg(any(target_os = "macos", test))]
    fn is_network_fs(kind: &str) -> bool {
        NETWORK_FS.contains(&kind)
    }

    /// This machine's table: on macOS the mount list (`getfsstat` with `MNT_NOWAIT`, which
    /// answers from the kernel's table and asks no server), on Windows the remote drives
    /// (`GetDriveTypeW` on each drive root, which reads the mount table only).
    pub(crate) fn current() -> Mounts {
        os::current()
    }
}

/// Whether `path` is on the network by its text and `mounts`, with no file call: one starting
/// with two separators (`\\host\share`, `//host/share`, `\\?\UNC\…`, `\\.\…`) other than a
/// verbatim drive path (`\\?\C:\…`), one on a remote drive letter, or one whose longest
/// mount-point prefix is a network file system.
pub(crate) fn is_network_path(path: &Path, mounts: &Mounts) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    let sep = |b: &u8| *b == b'/' || *b == b'\\';
    let mut rest = bytes;
    if bytes.get(..2).is_some_and(|two| two.iter().all(sep)) {
        match bytes.strip_prefix(br"\\?\") {
            Some(tail) if drive_letter(tail).is_some() => rest = tail,
            _ => return true,
        }
    }
    if let Some(letter) = drive_letter(rest) {
        return mounts.remote_drives.contains(&letter.to_ascii_uppercase());
    }
    mounts
        .points
        .iter()
        .filter(|(point, _)| under(path, point))
        .max_by_key(|(point, _)| point.components().count())
        .is_some_and(|(_, network)| *network)
}

/// The drive letter `bytes` starts with (`X:`), if any.
fn drive_letter(bytes: &[u8]) -> Option<u8> {
    match bytes {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => Some(*letter),
        _ => None,
    }
}

/// `path` with `.` dropped and each `..` taking off the name before it, by text alone: where
/// the path points if no link is on the way, for the mount check before any file call.
pub(crate) fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other @ (Component::Prefix(_) | Component::RootDir | Component::Normal(_)) => {
                out.push(other);
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "FFI to read the mount table with getfsstat")]
mod os {
    use std::ffi::CStr;
    use std::path::PathBuf;

    use super::Mounts;

    pub(super) fn current() -> Mounts {
        // SAFETY: a null buffer asks only for the count of mounts.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        let Ok(count) = usize::try_from(count) else {
            tracing::warn!("the matter walk could not read the mount list");
            return Mounts::default();
        };
        // Room for a few mounts made between the two calls.
        let room = count + 8;
        // SAFETY: statfs is plain old data, for which all zeroes is a valid value.
        let mut list: Vec<libc::statfs> = vec![unsafe { std::mem::zeroed() }; room];
        let Ok(bytes) = i32::try_from(room * size_of::<libc::statfs>()) else {
            return Mounts::default();
        };
        // SAFETY: `list` holds `bytes` bytes of writable statfs records.
        let got = unsafe { libc::getfsstat(list.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
        let Ok(got) = usize::try_from(got) else {
            tracing::warn!("the matter walk could not read the mount list");
            return Mounts::default();
        };
        list.truncate(got.min(room));
        let text = |name: &[libc::c_char]| {
            // SAFETY: the kernel NUL-terminates both names within their arrays.
            unsafe { CStr::from_ptr(name.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        };
        let points = list
            .iter()
            .map(|mount| {
                let kind = text(&mount.f_fstypename);
                (
                    PathBuf::from(text(&mount.f_mntonname)),
                    Mounts::is_network_fs(&kind),
                )
            })
            .collect();
        Mounts {
            points,
            remote_drives: Vec::new(),
        }
    }
}

#[cfg(windows)]
#[expect(unsafe_code, reason = "FFI to read the drive letters and their types")]
mod os {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::Win32::System::WindowsProgramming::DRIVE_REMOTE;
    use windows::core::PCWSTR;

    use super::Mounts;

    pub(super) fn current() -> Mounts {
        // SAFETY: no arguments; returns a bit mask of the drive letters in use.
        let drives = unsafe { GetLogicalDrives() };
        let remote_drives = (b'A'..=b'Z')
            .zip(0..26_u32)
            .filter(|(_, bit)| drives & (1 << bit) != 0)
            .filter(|(letter, _)| {
                let root = [u16::from(*letter), u16::from(b':'), u16::from(b'\\'), 0];
                // SAFETY: `root` is NUL-terminated UTF-16 that outlives the call.
                unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) == DRIVE_REMOTE }
            })
            .map(|(letter, _)| letter)
            .collect();
        Mounts {
            points: Vec::new(),
            remote_drives,
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod os {
    use super::Mounts;

    pub(super) fn current() -> Mounts {
        Mounts::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(path: &str, mounts: &Mounts) -> bool {
        is_network_path(Path::new(path), mounts)
    }

    #[test]
    fn server_and_device_paths_are_network_paths_by_their_text() {
        let none = Mounts::default();
        for path in [
            r"\\host\share",
            r"\\host\share\repo\.git",
            "//host/share/repo",
            r"\\?\UNC\host\share",
            r"\\.\pipe\x",
            r"\\?\Volume{0}\x",
            r"/\host\share",
        ] {
            assert!(net(path, &none), "{path}");
        }
        for path in [
            r"\\?\C:\repo",
            "/Users/a/repo",
            "repo/.git",
            r"C:\repo",
            "/",
        ] {
            assert!(!net(path, &none), "{path}");
        }
    }

    #[test]
    fn a_remote_drive_letter_is_a_network_path_in_any_case_and_form() {
        let mounts = Mounts::new(Vec::new(), b"z");
        for path in [r"Z:\repo", r"z:\repo", "Z:/repo", r"\\?\Z:\repo", "Z:repo"] {
            assert!(net(path, &mounts), "{path}");
        }
        for path in [r"C:\repo", r"\\?\C:\repo"] {
            assert!(!net(path, &mounts), "{path}");
        }
    }

    #[test]
    fn the_longest_mount_point_prefix_decides() {
        let mounts = Mounts::new(
            vec![
                (PathBuf::from("/"), false),
                (PathBuf::from("/Volumes/share"), true),
                (PathBuf::from("/Volumes/share/local"), false),
                (PathBuf::from("/net"), true),
            ],
            &[],
        );
        assert!(net("/Volumes/share", &mounts));
        assert!(net("/Volumes/share/repo/.git", &mounts));
        assert!(net("/net/host/x", &mounts));
        assert!(!net("/Volumes/share/local/x", &mounts));
        assert!(!net("/Volumes/shared/x", &mounts));
        assert!(!net("/Users/a", &mounts));
        assert!(!net("/Users/a", &Mounts::default()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_a_mount_point_matches_in_any_letter_case() {
        let mounts = Mounts::new(vec![(PathBuf::from("/Volumes/Share"), true)], &[]);
        assert!(net("/volumes/share/x", &mounts));
    }

    #[test]
    fn the_network_file_systems_are_named() {
        for kind in NETWORK_FS {
            assert!(Mounts::is_network_fs(kind), "{kind}");
        }
        for kind in ["apfs", "hfs", "devfs", "msdos", "exfat"] {
            assert!(!Mounts::is_network_fs(kind), "{kind}");
        }
    }

    #[test]
    fn lexical_drops_dots_and_never_climbs_past_the_root() {
        assert_eq!(lexical(Path::new("/a/b/../c/./d")), Path::new("/a/c/d"));
        assert_eq!(lexical(Path::new("/a/../../..")), Path::new("/"));
        assert_eq!(
            lexical(Path::new("/m/.git/worktrees/w/../../../../Volumes/s")),
            Path::new("/Volumes/s")
        );
    }
}
