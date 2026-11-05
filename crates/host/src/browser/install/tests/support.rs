//! What the download's tests share: a zip builder, a version list, and fake tools.

use std::path::Path;
use std::sync::Mutex;

use super::super::{Cancel, Failure, InstallTools};
use crate::browser::chrome::Platform;
use crate::write;

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

/// A zip of `entries`, stored (no compression), with no CRC: the checks never read either.
pub(super) fn zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let offset = out.len();
        u32_le(&mut out, 0x0403_4b50);
        u16_le(&mut out, 20);
        out.extend_from_slice(&[0; 12]); // flags, method, time, date, crc
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
        central.extend_from_slice(&[0; 12]); // flags, method, time, date, crc
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

/// A version list whose Stable is `version`, with `url` as the `mac-arm64` chrome zip.
pub(super) fn versions(version: &str, url: &str) -> Vec<u8> {
    serde_json::json!({
        "timestamp": "2026-11-02T00:00:00.000Z",
        "channels": {
            "Stable": {
                "channel": "Stable",
                "version": version,
                "revision": "1",
                "downloads": {
                    "chrome": [
                        {"platform": "linux64", "url": "https://example.com/linux.zip"},
                        {"platform": "mac-arm64", "url": url},
                    ],
                },
            },
        },
    })
    .to_string()
    .into_bytes()
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

/// Fake tools: a fixed free space, version list, zip, `Content-Length`, unpack and signature.
pub(super) struct Fake {
    pub(super) free: u64,
    pub(super) versions: Vec<u8>,
    pub(super) zip: Vec<u8>,
    /// The `Content-Length` the download reports.
    pub(super) length: Length,
    pub(super) made: Vec<Made>,
    pub(super) signed: Result<(), String>,
    /// The download sets the cancel halfway, as the lawyer's click would, and the tool ends.
    pub(super) cancel_in_download: bool,
    /// Each tool called, in order.
    pub(super) calls: Mutex<Vec<&'static str>>,
}

impl Fake {
    /// A download that works: Stable is [`VERSION`], the zip holds the Mac browser.
    pub(super) fn working() -> Self {
        Self {
            free: u64::MAX,
            versions: versions(VERSION, &google_url(VERSION)),
            zip: zip(&[file(MAC_BINARY, b"chrome")]),
            length: Length::Right,
            made: vec![Made::File(MAC_BINARY)],
            signed: Ok(()),
            cancel_in_download: false,
            calls: Mutex::new(Vec::new()),
        }
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

    fn fetch_versions(
        &self,
        _url: &str,
        _work: &Path,
        cancel: &Cancel,
    ) -> Result<Vec<u8>, Failure> {
        self.call("fetch_versions");
        cancel.check()?;
        Ok(self.versions.clone())
    }

    fn download(
        &self,
        _url: &str,
        zip: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(u64, u64),
    ) -> Result<Option<u64>, Failure> {
        self.call("download");
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

    fn unpack(&self, _zip: &Path, into: &Path, _cancel: &Cancel) -> Result<(), Failure> {
        self.call("unpack");
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
