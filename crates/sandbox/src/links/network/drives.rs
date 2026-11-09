//! Windows drive letters: what each one's DOS device is, read from the object manager's table
//! (`QueryDosDeviceW`), which asks no file system or server. Not `GetDriveTypeW`, which may try
//! to reconnect a disconnected mapped drive.

#[cfg(any(windows, test))]
use std::io;

/// What a drive letter is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(any(windows, test)),
    expect(dead_code, reason = "drive letters are read on Windows only")
)]
pub(crate) enum Drive {
    /// A local disk volume, a known local virtual disk (`VeraCrypt`, `ImDisk` and its RAM disks),
    /// or a letter with no device (nothing there to touch).
    Local,
    /// A redirector: SMB, `WebDAV`, offline files, Remote Desktop, a `subst` onto a UNC path.
    Network,
    /// A device not known to be either (rclone or another `WinFsp` drive, Google Drive, Box, a
    /// CD drive): never touched. A matter, a settings path or a git dir on one refuses the
    /// launch, since it may reach a server and nothing proves the agent can't write it.
    Unknown,
    /// A `subst` onto this path, as the device names it after `\??\` (`C:\work`): a local name
    /// for another path, which decides.
    Subst(String),
}

/// The device prefixes of local disk volumes and known local virtual disks, compared without
/// case. A VHD mounted from a share is a `HarddiskVolume` too (a known limit, 0049).
#[cfg(any(windows, test))]
const LOCAL_DEVICES: [&str; 3] = [
    r"\Device\HarddiskVolume",
    r"\Device\VeraCryptVolume",
    r"\Device\ImDisk",
];

/// The device prefixes of redirectors, compared without case.
#[cfg(any(windows, test))]
const NETWORK_DEVICES: [&str; 8] = [
    r"\Device\Mup",
    r"\Device\LanmanRedirector",
    r"\Device\WebDavRedirector",
    r"\Device\CSCDevice",
    r"\Device\RdpDr",
    r"\Device\VBoxMiniRdr",
    r"\Device\P9Rdr",
    r"\??\UNC",
];

/// What the DOS device `target` is, by its text alone.
#[cfg(any(windows, test))]
pub(crate) fn drive(target: &str) -> Drive {
    let starts = |prefix: &str| {
        target
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    if NETWORK_DEVICES.iter().any(|prefix| starts(prefix)) {
        return Drive::Network;
    }
    if LOCAL_DEVICES.iter().any(|prefix| starts(prefix)) {
        return Drive::Local;
    }
    let Some(path) = target.strip_prefix(r"\??\") else {
        return Drive::Unknown;
    };
    match path.as_bytes() {
        [letter, b':', ..] if letter.is_ascii_alphabetic() => Drive::Subst(path.to_owned()),
        _ => Drive::Unknown,
    }
}

/// Every drive letter that has a device, upper case, with what it is, by `lookup` (a letter's
/// DOS device target, `None` when it has none). A failed lookup is [`Drive::Unknown`].
#[cfg(any(windows, test))]
pub(crate) fn drive_table(lookup: impl Fn(u8) -> io::Result<Option<String>>) -> Vec<(u8, Drive)> {
    (b'A'..=b'Z')
        .filter_map(|letter| match lookup(letter) {
            Ok(Some(target)) => Some((letter, drive(&target))),
            Ok(None) => None,
            Err(_) => Some((letter, Drive::Unknown)),
        })
        .collect()
}

#[cfg(windows)]
#[expect(unsafe_code, reason = "FFI to read the drive letters' DOS devices")]
pub(crate) mod os {
    use std::io;

    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;
    use windows::core::PCWSTR;

    use super::{Drive, drive_table};

    /// This machine's drive letters.
    pub(crate) fn table() -> Vec<(u8, Drive)> {
        drive_table(dos_device)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_device_is_local_only_when_a_disk_volume() {
        for target in [
            r"\Device\HarddiskVolume3",
            r"\device\harddiskvolume1",
            r"\Device\VeraCryptVolumeV",
            r"\Device\ImDisk0",
        ] {
            assert_eq!(drive(target), Drive::Local, "{target}");
        }
        for target in [
            r"\Device\Mup\;Z:000\host\share",
            r"\Device\LanmanRedirector\;Z:0\host\share",
            r"\Device\WebDavRedirector\;W:\host\x",
            r"\Device\CSCDevice\x",
            r"\Device\RdpDr\;:1\tsclient\C",
            r"\??\UNC\host\share",
            r"\??\unc\host\share",
        ] {
            assert_eq!(drive(target), Drive::Network, "{target}");
        }
        for target in [
            r"\Device\Volume{d6cc17c5-1734-4085-bce7-964f1e9f5de9}",
            r"\Device\WinFsp.Disk",
            r"\Device\CdRom0",
            r"\Device\Floppy0",
            r"\??\Volume{0}",
            "",
        ] {
            assert_eq!(drive(target), Drive::Unknown, "{target}");
        }
        assert_eq!(
            drive(r"\??\c:\work\state"),
            Drive::Subst(r"c:\work\state".to_owned())
        );
    }

    #[test]
    fn the_table_lists_letters_with_a_device_and_fails_closed() {
        let table = drive_table(|letter| match letter {
            b'C' => Ok(Some(r"\Device\HarddiskVolume3".to_owned())),
            b'V' => Ok(Some(r"\Device\Volume{0}".to_owned())),
            b'X' => Ok(Some(r"\??\C:\work".to_owned())),
            b'Q' => Err(io::Error::other("refused")),
            _ => Ok(None),
        });
        assert_eq!(
            table,
            [
                (b'C', Drive::Local),
                (b'Q', Drive::Unknown),
                (b'V', Drive::Unknown),
                (b'X', Drive::Subst(r"C:\work".to_owned())),
            ]
        );
    }
}
