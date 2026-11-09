//! What the download's tests share: a zip builder, pins, and fake tools.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;

use sha2::Digest as _;

use super::super::system::run_tool;
use super::super::{Cancel, Failure, InstallTools};
use crate::browser::chrome::Platform;
use crate::browser::pins::{Pin, Pins};
use crate::write;

/// The system's unzip, which the link tests unpack with when it is there.
pub(super) const UNZIP: &str = "/usr/bin/unzip";

/// The version every test downloads.
pub(super) const VERSION: &str = "131.0.6778.85";

/// The browser's path inside the unpacked folder on Apple silicon.
pub(super) const MAC_BINARY: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";

/// One zip entry: its name, its bytes, and a Unix mode (`0o120777` makes it a link).
pub(super) struct ZipEntry<'a> {
    pub(super) name: &'a str,
    pub(super) bytes: &'a [u8],
    pub(super) mode: Option<u32>,
}

/// A file entry.
pub(super) fn file<'a>(name: &'a str, bytes: &'a [u8]) -> ZipEntry<'a> {
    ZipEntry {
        name,
        bytes,
        mode: None,
    }
}

fn u16_le(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&u16::try_from(value).unwrap().to_le_bytes());
}

fn u32_le(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&u32::try_from(value).unwrap().to_le_bytes());
}

/// The CRC-32 (IEEE) of `bytes`, so a real unzip takes the zip.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A zip of `entries`, stored (no compression), with their CRCs.
pub(super) fn zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = out.len();
        let crc = crc32(entry.bytes).to_le_bytes();
        u32_le(&mut out, 0x0403_4b50);
        u16_le(&mut out, 20);
        out.extend_from_slice(&[0; 8]); // flags, method, time, date
        out.extend_from_slice(&crc);
        u32_le(&mut out, entry.bytes.len());
        u32_le(&mut out, entry.bytes.len());
        u16_le(&mut out, entry.name.len());
        u16_le(&mut out, 0);
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(entry.bytes);

        u32_le(&mut central, 0x0201_4b50);
        let made_by = if entry.mode.is_some() { 0x031e } else { 20 };
        u16_le(&mut central, made_by);
        u16_le(&mut central, 20);
        central.extend_from_slice(&[0; 8]); // flags, method, time, date
        central.extend_from_slice(&crc);
        u32_le(&mut central, entry.bytes.len());
        u32_le(&mut central, entry.bytes.len());
        u16_le(&mut central, entry.name.len());
        central.extend_from_slice(&[0; 8]); // extra, comment, disk, internal
        let external = entry.mode.map_or(0, |mode| (mode as usize) << 16);
        u32_le(&mut central, external);
        u32_le(&mut central, offset);
        central.extend_from_slice(entry.name.as_bytes());
    }
    let central_at = out.len();
    out.extend_from_slice(&central);
    u32_le(&mut out, 0x0605_4b50);
    out.extend_from_slice(&[0; 4]);
    u16_le(&mut out, entries.len());
    u16_le(&mut out, entries.len());
    u32_le(&mut out, central.len());
    u32_le(&mut out, central_at);
    u16_le(&mut out, 0);
    out
}

/// The pin of `zip`: its sha256 and size.
pub(super) fn pin_of(zip: &[u8]) -> Pin {
    let digest = sha2::Sha256::digest(zip);
    Pin {
        sha256: digest.iter().fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").unwrap();
            hex
        }),
        size: u64::try_from(zip.len()).unwrap(),
    }
}

/// Pins of [`VERSION`] whose `chrome` zip for Apple silicon and Windows x64 is `pin`.
pub(super) fn pins_with(pin: &Pin) -> Pins {
    Pins::for_tests(
        "2026-11-07T06:00:00Z",
        VERSION,
        &[
            ("chrome", "mac-arm64", pin.clone()),
            ("chrome", "win64", pin.clone()),
        ],
    )
}

/// Google's URL for `version`'s `mac-arm64` zip.
pub(super) fn google_url(version: &str) -> String {
    format!(
        "https://storage.googleapis.com/chrome-for-testing-public/{version}/mac-arm64/\
         chrome-mac-arm64.zip"
    )
}

/// What the fake unpack makes, under the unpack folder.
pub(super) enum Made {
    File(&'static str),
    #[cfg(unix)]
    Link(&'static str, &'static str),
}

/// The `Content-Length` the fake download reports.
#[derive(Clone, Copy)]
pub(super) enum Length {
    /// The zip's own size.
    Right,
    /// This size.
    Wrong(u64),
    /// None.
    Missing,
}

/// Fake tools: a fixed free space, zip, `Content-Length`, unpack and signature.
pub(super) struct Fake {
    pub(super) free: u64,
    pub(super) zip: Vec<u8>,
    /// The `Content-Length` the download reports.
    pub(super) length: Length,
    pub(super) made: Vec<Made>,
    pub(super) signed: Result<(), String>,
    /// The download sets the cancel halfway, as the lawyer's click would, and the tool ends.
    pub(super) cancel_in_download: bool,
    /// The unpack runs the system's `/usr/bin/unzip` on the zip, in place of `made`.
    pub(super) unzip: bool,
    /// Each tool called, in order.
    pub(super) calls: Mutex<Vec<&'static str>>,
    /// Each download's URL and size cap.
    pub(super) downloads: Mutex<Vec<(String, u64)>>,
}

impl Fake {
    /// A download that works: the zip holds the Mac browser ([`Fake::pins`] pin it).
    pub(super) fn working() -> Self {
        Self {
            free: u64::MAX,
            zip: zip(&[file(MAC_BINARY, b"chrome")]),
            length: Length::Right,
            made: vec![Made::File(MAC_BINARY)],
            signed: Ok(()),
            cancel_in_download: false,
            unzip: false,
            calls: Mutex::new(Vec::new()),
            downloads: Mutex::new(Vec::new()),
        }
    }

    /// Pins of [`VERSION`] pinning this fake's zip.
    pub(super) fn pins(&self) -> Pins {
        pins_with(&pin_of(&self.zip))
    }

    pub(super) fn downloads(&self) -> Vec<(String, u64)> {
        self.downloads.lock().unwrap().clone()
    }

    pub(super) fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn call(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
}

impl InstallTools for Fake {
    fn free_bytes(&self, _folder: &Path) -> std::io::Result<u64> {
        Ok(self.free)
    }

    fn download(
        &self,
        url: &str,
        zip: &Path,
        max_bytes: u64,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
        self.call("download");
        self.downloads
            .lock()
            .unwrap()
            .push((url.to_owned(), max_bytes));
        let total = u64::try_from(self.zip.len()).unwrap();
        let half = self.zip.len() / 2;
        write::write_file(zip, &self.zip[..half]).unwrap();
        progress(u64::try_from(half).unwrap(), total);
        if self.cancel_in_download {
            cancel.cancel();
            return Err(Failure::cancelled());
        }
        write::write_file(zip, &self.zip).unwrap();
        progress(total, total);
        Ok(match self.length {
            Length::Right => Some(total),
            Length::Wrong(length) => Some(length),
            Length::Missing => None,
        })
    }

    fn unpack(&self, zip: &Path, into: &Path, cancel: &Cancel) -> Result<(), Failure> {
        self.call("unpack");
        if self.unzip {
            let args = vec!["-q".into(), zip.into(), "-d".into(), into.into()];
            let ran = run_tool(Path::new(UNZIP), args, into, cancel, 0, &mut || Ok(()))
                .map_err(|_stop| Failure::cancelled())
                .unwrap();
            assert_eq!(ran.code, Some(0), "unzip: {}", ran.stderr);
            return Ok(());
        }
        for made in &self.made {
            match made {
                Made::File(name) => {
                    let path = into.join(name);
                    write::create_dir_all(path.parent().unwrap()).unwrap();
                    write::write_file(&path, b"chrome").unwrap();
                }
                #[cfg(unix)]
                Made::Link(name, target) => {
                    write::symlink(Path::new(target), &into.join(name)).unwrap();
                }
            }
        }
        Ok(())
    }

    fn verify_signature(
        &self,
        _platform: Platform,
        _folder: &Path,
        _cancel: &Cancel,
    ) -> Result<(), String> {
        self.call("verify_signature");
        self.signed.clone()
    }
}
