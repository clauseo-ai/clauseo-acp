//! Windows drive letters: what each one is, by two calls that open nothing on the drive: its
//! DOS device (`QueryDosDeviceW`, the object manager's table) and, for a device no redirector
//! prefix names, `GetDriveTypeW`. Only a positively identified network drive is refused; any
//! other device is local (decision 0049, "Which drives count as network").
//!
//! `GetDriveTypeW` can try to reconnect a disconnected mapped drive, so it is never asked about
//! a letter with no DOS device (an Unavailable mapping) or one whose device is a redirector's.

#[cfg(any(windows, test))]
use std::io;

/// What a drive letter is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "drive letters are read on Windows only")
)]
pub(crate) enum Drive {
    /// Not positively network: a disk volume, `VeraCrypt`, `ImDisk`, a CD, a RAM disk, or a
    /// device the launch doesn't know (rclone or another `WinFsp` drive in disk mode). A
    /// letter with no device counts as local too: nothing there to touch.
    Local,
    /// A redirector by its device's name (SMB, `WebDAV`, offline files, Remote Desktop, the
    /// VM shared-folder redirectors, a `subst` onto a UNC path), or `DRIVE_REMOTE`.
    Network,
    /// A `subst` onto this path, as the device names it after `\??\` (`C:\work`): a local name
    /// for another path, which decides.
    Subst(String),
}

/// The device prefixes of redirectors, compared without case. Sources are in decision 0049.
#[cfg(any(windows, test))]
pub(crate) const REDIRECTORS: [&str; 10] = [
    r"\Device\Mup",
    r"\Device\LanmanRedirector",
    r"\Device\WebDavRedirector",
    r"\Device\CSCDevice",
    r"\Device\RdpDr",
    r"\Device\P9Rdr",
    r"\Device\VBoxMiniRdr",
    r"\Device\hgfs",
    r"\Device\PrlMiniRdr",
    r"\??\UNC\",
];

/// `GetDriveTypeW`'s answers the table reads.
#[cfg(any(windows, test))]
pub(crate) mod kind {
    /// `DRIVE_NO_ROOT_DIR`: nothing mounted there.
    pub(crate) const NO_ROOT_DIR: u32 = 1;
    /// `DRIVE_REMOTE`.
    pub(crate) const REMOTE: u32 = 4;
}

/// What a drive letter with DOS device `target` is, by the target's text: a redirector's,
/// a `subst` onto a drive path, or `None` when only `GetDriveTypeW` can say.
#[cfg(any(windows, test))]
fn by_device(target: &str) -> Option<Drive> {
    let starts = |prefix: &str| {
        target
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    if REDIRECTORS.iter().any(|prefix| starts(prefix)) {
        return Some(Drive::Network);
    }
    let path = target.strip_prefix(r"\??\")?;
    match path.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => Some(Drive::Subst(path.to_owned())),
        _ => None,
    }
}

/// What a drive letter is by `GetDriveTypeW`'s answer: `None` when nothing is mounted there.
#[cfg(any(windows, test))]
fn by_kind(drive_type: u32) -> Option<Drive> {
    match drive_type {
        kind::REMOTE => Some(Drive::Network),
        kind::NO_ROOT_DIR => None,
        // Fixed, removable, CD, RAM disk, or unknown: not positively network.
        _ => Some(Drive::Local),
    }
}

/// Every drive letter that has a device, upper case, with what it is.
///
/// - `dos_device`: a letter's DOS device target, `None` when it has none. A letter with none is
///   missing and left out, and `GetDriveTypeW` is not asked.
/// - `drive_type`: `GetDriveTypeW` of the letter's root, asked only when the device names no
///   redirector and no `subst`, or the device couldn't be read.
#[cfg(any(windows, test))]
pub(crate) fn drive_table(
    dos_device: impl Fn(u8) -> io::Result<Option<String>>,
    drive_type: impl Fn(u8) -> u32,
) -> Vec<(u8, Drive)> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let drive = match dos_device(letter) {
                Ok(None) => None,
                Ok(Some(target)) => by_device(&target).or_else(|| by_kind(drive_type(letter))),
                Err(_) => by_kind(drive_type(letter)),
            };
            drive.map(|drive| (letter, drive))
        })
        .collect()
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI to read the drive letters' DOS devices and drive types"
)]
pub(crate) mod os {
    use std::io;

    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, QueryDosDeviceW};
    use windows::Win32::System::WindowsProgramming::{DRIVE_NO_ROOT_DIR, DRIVE_REMOTE};
    use windows::core::PCWSTR;

    use super::{Drive, drive_table, kind};

    const _: () = assert!(kind::REMOTE == DRIVE_REMOTE && kind::NO_ROOT_DIR == DRIVE_NO_ROOT_DIR);

    /// This machine's drive letters.
    pub(crate) fn table() -> Vec<(u8, Drive)> {
        drive_table(dos_device, drive_type)
    }

    /// The first target of the DOS device `X:` for `letter`, `None` if there is none.
    fn dos_device(letter: u8) -> io::Result<Option<String>> {
        let name = [u16::from(letter), u16::from(b':'), 0];
        let mut target = [0_u16; 1024];
        // SAFETY: `name` is NUL-terminated UTF-16 that outlives the call, and `target` is a
        // writable buffer whose length the binding passes.
        let written = unsafe { QueryDosDeviceW(PCWSTR(name.as_ptr()), Some(&mut target)) };
        if written == 0 {
            // SAFETY: no arguments; reads this thread's last error.
            let error = unsafe { GetLastError() };
            if error == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            return Err(io::Error::other("QueryDosDeviceW failed"));
        }
        let written = usize::try_from(written).map_err(io::Error::other)?;
        let list = target.get(..written).unwrap_or_default();
        let first = list.split(|unit| *unit == 0).next().unwrap_or_default();
        String::from_utf16(first)
            .map(Some)
            .map_err(io::Error::other)
    }

    /// `GetDriveTypeW` of `X:\` for `letter`.
    fn drive_type(letter: u8) -> u32 {
        let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
        // SAFETY: `root` is NUL-terminated UTF-16 that outlives the call.
        unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// `GetDriveTypeW`'s other answers.
    const UNKNOWN: u32 = 0;
    const REMOVABLE: u32 = 2;
    const FIXED: u32 = 3;
    const CDROM: u32 = 5;
    const RAMDISK: u32 = 6;

    /// What drive letter `D:` is with DOS device `target` and drive type `kind`, and whether
    /// `GetDriveTypeW` was asked.
    fn one(target: io::Result<Option<&str>>, kind: u32) -> (Option<Drive>, bool) {
        let asked = RefCell::new(false);
        let target = target.map(|target| target.map(str::to_owned));
        let table = drive_table(
            |letter| match (letter, &target) {
                (b'D', Ok(target)) => Ok(target.clone()),
                (b'D', Err(error)) => Err(io::Error::new(error.kind(), "refused")),
                _ => Ok(None),
            },
            |letter| {
                assert_eq!(letter, b'D');
                *asked.borrow_mut() = true;
                kind
            },
        );
        let drive = table.into_iter().find(|(letter, _)| *letter == b'D');
        (drive.map(|(_, drive)| drive), asked.into_inner())
    }

    /// Every redirector prefix is network by its name alone, in any case, without asking
    /// `GetDriveTypeW` (which could reconnect a disconnected mapping).
    #[test]
    fn a_redirector_device_is_network_without_asking_the_drive_type() {
        for prefix in REDIRECTORS {
            for target in [
                format!(r"{prefix}\;D:0000000000012345\host\share"),
                prefix.to_lowercase(),
                prefix.to_uppercase(),
            ] {
                for kind in [FIXED, UNKNOWN] {
                    let (drive, asked) = one(Ok(Some(&target)), kind);
                    assert_eq!(drive, Some(Drive::Network), "{target}");
                    assert!(!asked, "{target}");
                }
            }
        }
    }

    /// The decision table over a drive letter's DOS device and drive type: network only when the
    /// device is a redirector's (above) or the drive type is `DRIVE_REMOTE`; missing when there
    /// is no device or no root; every other device local, whatever its drive type.
    #[test]
    fn a_drive_is_network_only_when_positively_identified() {
        let cases = [
            (
                Ok(Some(r"\Device\HarddiskVolume3")),
                FIXED,
                Some(Drive::Local),
                true,
            ),
            (
                Ok(Some(r"\Device\HarddiskVolume3")),
                UNKNOWN,
                Some(Drive::Local),
                true,
            ),
            (
                Ok(Some(r"\Device\HarddiskVolume3")),
                kind::REMOTE,
                Some(Drive::Network),
                true,
            ),
            (
                Ok(Some(r"\Device\HarddiskVolume3")),
                kind::NO_ROOT_DIR,
                None,
                true,
            ),
            (
                Ok(Some(r"\Device\WinFsp.Disk")),
                kind::REMOTE,
                Some(Drive::Network),
                true,
            ),
            (
                Ok(Some(r"\Device\WinFsp.Disk")),
                FIXED,
                Some(Drive::Local),
                true,
            ),
            (
                Ok(Some(r"\Device\VeraCryptVolumeV")),
                REMOVABLE,
                Some(Drive::Local),
                true,
            ),
            (Ok(Some(r"\Device\CdRom0")), CDROM, Some(Drive::Local), true),
            (
                Ok(Some(r"\Device\ImDisk0")),
                RAMDISK,
                Some(Drive::Local),
                true,
            ),
            (Ok(None), kind::REMOTE, None, false),
            (Err(io::Error::other("x")), FIXED, Some(Drive::Local), true),
            (
                Err(io::Error::other("x")),
                kind::REMOTE,
                Some(Drive::Network),
                true,
            ),
            (
                Ok(Some(r"\??\c:\work\state")),
                kind::REMOTE,
                Some(Drive::Subst(r"c:\work\state".to_owned())),
                false,
            ),
        ];
        for (target, kind, want, want_asked) in cases {
            let shown = format!("{target:?} {kind}");
            let (drive, asked) = one(target, kind);
            assert_eq!((drive, asked), (want, want_asked), "{shown}");
        }
    }
}
