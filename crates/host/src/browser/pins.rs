//! The signed browser pins: which Chrome for Testing version Clauseo downloads, and each zip's
//! sha256 and size, published by stage F as a JSON manifest with a minisign signature
//! (`browser-pins-v1.json` and its `.minisig`, [`PINS_URL`]).
//!
//! - Signature: minisign over the exact JSON bytes, checked by the block list's code
//!   (`clauseo_blocklist::trusted_comment`) against the pins' own keys ([`PINS_PUBLIC_KEYS`]:
//!   the current key and the next one, so a rotation needs no emergency release). The trusted
//!   comment is exactly `clauseo-browser-pins v1 published=<RFC 3339> version=<version>
//!   run=<digits>`, and its `published` and `version` must equal the body's (`parse.rs`).
//! - Body: `schema` 1 (anything else is refused), `published`, and `stable` with a `version`
//!   of dotted digits and `downloads.<name>.<platform>` = `{sha256, size}`: 64 lowercase hex
//!   digits, and 1 to [`MAX_ZIP_BYTES`] bytes. Fields it does not know are ignored. No URL is
//!   read from it: the zip's URL comes from a fixed template (`install::manifest::zip_url`).
//! - Where: the copy the app ships (`<resource dir>/browser-pins/`, which the desktop passes in,
//!   [`BrowserPins::bundled`]) and the one the `clauseo-blocklist` thread fetched and stored
//!   (`<data dir>/browser-pins/`, `blocklist/updater/pins.rs`). Readers (the launch prep, the
//!   download's thread) read both from disk each time ([`PinsSource::newest`]) and use the
//!   newer that verifies; with neither, the downloaded Chrome for Testing is refused
//!   (`BrowserListUnavailable`) and the download too (`list_unavailable`).
//! - Floor record (`floor.rs`): beside the stored pair, a host-owned file with the `published`
//!   and `version` of the newest manifest ever stored, written before the pair. While no copy
//!   that verifies stands at or above it (the stored pair broken or gone), readers use none,
//!   rather than the lower bundled copy, until a fetch stores one at or above it.
//! - Freshness: the used copy's signed `published` against the host clock. Over 14 days the
//!   downloaded browser carries `BrowserListOld`; over 45 days it is refused
//!   (`BrowserListExpired`) and the download too (`list_unavailable`). The bundled copy counts
//!   by its own date. A daily heartbeat (same version, newer date) keeps it fresh.
//! - No rollback ([`accept`]): a fetched manifest is stored only when it stands at or above the
//!   floor record and is the same bytes as the newest verified copy the app has, at any date
//!   ([`PinsSource::floor`]), or is published strictly later and pins no lower version.
//! - Clock set back: when the newest verified copy, or the floor record, is dated over a day
//!   ahead of the host clock, readers use no copy at all (`BrowserListUnavailable`), never the
//!   lower one: setting the clock back lowers nothing.

use std::collections::BTreeMap;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use clauseo_blocklist::Generated;
use clauseo_protocol::{Timestamp, ToolNotice};

use crate::blocklist::DAY;
use crate::write;

pub mod floor;
mod parse;

pub use floor::Floor;
pub use parse::PinsError;

/// The current key's minisign public key file (key id C5496A136395A5B3), as in
/// `clauseo-ai/browser-pins`'s `minisign.pub`. The secret key is only in that repo's secrets.
pub const PINS_KEY_CURRENT: &str = include_str!("../../keys/browser-pins/minisign.pub");

/// The next key's minisign public key file (key id B8557C44951AF117), as in
/// `clauseo-ai/browser-pins`'s `minisign-next.pub`: shipped before the publisher switches to it.
pub const PINS_KEY_NEXT: &str = include_str!("../../keys/browser-pins/minisign-next.pub");

/// Every key a browser pins manifest may be signed with. Not the block list's keys.
pub const PINS_PUBLIC_KEYS: &[&str] = &[PINS_KEY_CURRENT, PINS_KEY_NEXT];

/// The published manifest; its signature is beside it with `.minisig` added.
pub const PINS_URL: &str =
    "https://github.com/clauseo-ai/browser-pins/releases/download/current/browser-pins-v1.json";

/// The folder holding a manifest and its signature: under the data folder (the stored copy)
/// and under the app's resource folder (the bundled copy).
pub const PINS_FOLDER: &str = "browser-pins";

/// The manifest's file name, as published and as stored.
pub const MANIFEST_FILE: &str = "browser-pins-v1.json";

/// Its signature's file name.
pub const SIGNATURE_FILE: &str = "browser-pins-v1.json.minisig";

/// The largest manifest read, fetched or from disk: a real one is about 2 KB.
pub const MAX_MANIFEST: u64 = 64 * 1024;

/// The largest zip a pin may name, and so the most a download reads: 400 MiB.
pub const MAX_ZIP_BYTES: u64 = 400 * 1024 * 1024;

/// The download name of Chrome for Testing's full browser in the manifest and its zip's URL.
pub const DOWNLOAD_CHROME: &str = "chrome";

/// How old the newest manifest may be before the downloaded browser carries
/// [`ToolNotice::BrowserListOld`].
pub const LIST_OLD_AFTER_SECONDS: i64 = 14 * DAY;

/// How old the newest manifest may be before the downloaded browser is refused
/// (`BrowserListExpired`) and the download too (`list_unavailable`).
pub const LIST_EXPIRED_AFTER_SECONDS: i64 = 45 * DAY;

/// One zip's pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// Its sha256: 64 lowercase hex digits.
    pub sha256: String,
    /// Its size in bytes, 1 to [`MAX_ZIP_BYTES`].
    pub size: u64,
}

/// A manifest that verified: its signature, its trusted comment and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pins {
    published: Generated,
    version: String,
    /// `version` as numbers, for comparing.
    numbers: Vec<u64>,
    run: String,
    downloads: BTreeMap<String, BTreeMap<String, Pin>>,
    /// The sha256 of the manifest's exact bytes: the floor rule takes the same bytes again.
    digest: [u8; 32],
}

impl Pins {
    /// When stage F published it.
    pub fn published(&self) -> Generated {
        self.published
    }

    /// The Chrome for Testing version it pins (Stable), dotted digits.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The publishing run's number from the trusted comment: for the log only, never read.
    pub fn run(&self) -> &str {
        &self.run
    }

    /// The pin of download `name` (such as [`DOWNLOAD_CHROME`]) for Chrome for Testing's
    /// `platform` (`win64`, `mac-arm64`, ...), if the manifest has one.
    pub fn pin(&self, name: &str, platform: &str) -> Option<&Pin> {
        self.downloads.get(name)?.get(platform)
    }

    /// Verifies `minisig` over the exact bytes of `body` against `keys` (key files or bare
    /// key lines) and reads it (`parse.rs`): the manifest, or why it was refused.
    pub fn verify(body: &[u8], minisig: &[u8], keys: &[&str]) -> Result<Pins, PinsError> {
        parse::verify(body, minisig, keys)
    }

    /// Whether it is dated more than a day ahead of `now`.
    pub fn is_ahead_of(&self, now: Timestamp) -> bool {
        self.published.unix_seconds() > seconds(now) + DAY
    }

    /// Pins made in a test, without a signature.
    #[cfg(test)]
    pub(crate) fn for_tests(published: &str, version: &str, pins: &[(&str, &str, Pin)]) -> Pins {
        let mut downloads: BTreeMap<String, BTreeMap<String, Pin>> = BTreeMap::new();
        for (name, platform, pin) in pins {
            downloads
                .entry((*name).to_owned())
                .or_default()
                .insert((*platform).to_owned(), pin.clone());
        }
        Pins {
            published: Generated::parse(published).unwrap(),
            version: version.to_owned(),
            numbers: crate::browser::chrome::version(version).unwrap(),
            run: "1".to_owned(),
            downloads,
            digest: [0; 32],
        }
    }
}

/// The floor rule: may `candidate` stand against `floor`, the newest verified manifest the
/// app has at any date (`PinsSource::floor`; `None`: none), and `record`, the floor record
/// (`None`: none)? Only when it stands at or above `record`, and is the same bytes as `floor`
/// or is published strictly later and pins no lower version. So a heartbeat (the same version,
/// published later) is taken and resets freshness, and a replay of an older or equally dated
/// manifest is refused, even while the stored pair is broken and only the bundled copy verifies.
pub fn accept(candidate: &Pins, floor: Option<&Pins>, record: Option<&Floor>) -> bool {
    record.is_none_or(|record| record.covered_by(candidate))
        && floor.is_none_or(|floor| {
            candidate.digest == floor.digest
                || (candidate.published > floor.published && candidate.numbers >= floor.numbers)
        })
}

/// Whether `candidate` is the same bytes as `other`.
pub fn same_bytes(candidate: &Pins, other: &Pins) -> bool {
    candidate.digest == other.digest
}

/// The newer of two verified manifests: the higher version, then the later date. So the
/// version in use never goes down, whichever of the two copies is dated later.
fn newer(a: Pins, b: Pins) -> Pins {
    if (&b.numbers, b.published) > (&a.numbers, a.published) {
        b
    } else {
        a
    }
}

/// [`newer`] of whichever of the two there are.
fn newer_of(a: Option<Pins>, b: Option<Pins>) -> Option<Pins> {
    match (a, b) {
        (Some(a), Some(b)) => Some(newer(a, b)),
        (a, b) => a.or(b),
    }
}

/// The notice of a browser that is a Chrome for Testing Clauseo downloaded, judged by the
/// newest manifest's `published`: [`ToolNotice::BrowserListOld`] when that is over 14 days
/// before `now`.
pub fn notice(published: Generated, now: Timestamp) -> Option<ToolNotice> {
    (seconds(now) - published.unix_seconds() > LIST_OLD_AFTER_SECONDS)
        .then_some(ToolNotice::BrowserListOld)
}

/// Whether the newest manifest, published at `published`, is over 45 days before `now`: then a
/// downloaded Chrome for Testing is refused and no download starts.
pub fn expired(published: Generated, now: Timestamp) -> bool {
    seconds(now) - published.unix_seconds() > LIST_EXPIRED_AFTER_SECONDS
}

/// `now` in Unix seconds.
fn seconds(now: Timestamp) -> i64 {
    now.as_millis() / 1000
}

/// Where the browser pins come from: the bundled copy's folder, the release's URLs and the
/// keys. In the host's config (`BrowserPins::apply`); the default is the release's with no
/// bundled copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserPins {
    /// The folder of the copy the app ships (`MANIFEST_FILE` and `SIGNATURE_FILE`), if any:
    /// `<resource dir>/browser-pins/` in a release build.
    pub bundled: Option<PathBuf>,
    /// The manifest's URL.
    pub manifest_url: String,
    /// The signature's URL.
    pub signature_url: String,
    /// The minisign keys (key files or bare key lines) a manifest may be signed with.
    pub keys: Vec<String>,
}

impl Default for BrowserPins {
    fn default() -> Self {
        Self::release(None)
    }
}

impl BrowserPins {
    /// The release's: [`PINS_URL`] and its `.minisig`, [`PINS_PUBLIC_KEYS`], with `bundled`.
    /// The fetch follows the block list's rules: redirects by hand to the release hosts,
    /// `https` only, `network.upstream_proxy` else the system's proxy.
    pub fn release(bundled: Option<PathBuf>) -> Self {
        Self {
            bundled,
            manifest_url: PINS_URL.to_owned(),
            signature_url: format!("{PINS_URL}.minisig"),
            keys: PINS_PUBLIC_KEYS
                .iter()
                .map(|key| (*key).to_owned())
                .collect(),
        }
    }

    /// `config` with these pins. Kept here, not as a `HostConfig` builder, so `config.rs`
    /// stays within 600 lines.
    pub fn apply(self, mut config: crate::HostConfig) -> crate::HostConfig {
        config.wiring.browser_pins = self;
        config
    }

    /// The readers' view, with the stored copy under `data_dir`.
    pub(crate) fn source(&self, data_dir: &Path) -> PinsSource {
        PinsSource {
            bundled: self.bundled.clone(),
            stored: data_dir.join(PINS_FOLDER),
            keys: self.keys.clone(),
        }
    }
}

/// The two copies a reader looks at, and the keys. File system calls: off the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinsSource {
    /// The bundled copy's folder, if any.
    pub bundled: Option<PathBuf>,
    /// The stored copy's folder, `<data dir>/browser-pins/`.
    pub stored: PathBuf,
    /// The keys.
    pub keys: Vec<String>,
}

impl PinsSource {
    /// The copy to use: the newer verified of the bundled and the stored copy ([`Self::floor`]).
    /// `None` (logged) when neither verifies; when that copy, or the floor record, is dated over
    /// a day ahead of `now` (the clock set back: the lower copy is not used instead); and when
    /// the floor record is unreadable or that copy stands below it (the stored pair broken: the
    /// lower bundled copy is not used instead).
    pub fn newest(&self, now: Timestamp) -> Option<Pins> {
        let Ok(record) = floor::read(&self.stored) else {
            tracing::warn!("the browser pins floor record is unreadable; no browser pins are used");
            return None;
        };
        let best = self.floor()?;
        if best.is_ahead_of(now)
            || record
                .as_ref()
                .is_some_and(|record| record.is_ahead_of(now))
        {
            tracing::warn!(
                published = best.published.as_str(),
                "the browser pins are dated over a day ahead of this computer's clock; none used"
            );
            return None;
        }
        if let Some(record) = record
            && !record.covered_by(&best)
        {
            tracing::warn!(
                floor = record.published().as_str(),
                published = best.published.as_str(),
                "no browser pins copy stands at or above the floor record; none used until a \
                 newer one is fetched"
            );
            return None;
        }
        Some(best)
    }

    /// The floor record beside the stored pair (`floor.rs`): `Ok(None)` when there is none.
    pub fn floor_record(&self) -> Result<Option<Floor>, floor::Unreadable> {
        floor::read(&self.stored)
    }

    /// The floor: the newer verified of the bundled and the stored copy WHATEVER
    /// their dates. The date only chooses which copy is used ([`Self::newest`]); it never lowers
    /// the floor, so with the clock set back a replayed older manifest cannot replace a stored
    /// one dated "ahead".
    pub fn floor(&self) -> Option<Pins> {
        let (bundled, stored) = self.verified();
        newer_of(bundled, stored)
    }

    /// The bundled and the stored copy, each if it verifies, at any date.
    fn verified(&self) -> (Option<Pins>, Option<Pins>) {
        let bundled = self
            .bundled
            .as_deref()
            .and_then(|folder| self.read(folder, "bundled"));
        (bundled, self.read(&self.stored, "stored"))
    }

    /// The verified copy in `folder`, or `None` (logged when its files are there but refused).
    fn read(&self, folder: &Path, which: &'static str) -> Option<Pins> {
        self.read_with(which, &mut || read_pair(folder))
    }

    /// [`Self::read`] with the pair read by `load`. A signature that does not verify is read
    /// again once: a store replaces the manifest, then its signature, so a read between the two
    /// sees the new manifest beside the old signature.
    fn read_with(
        &self,
        which: &'static str,
        load: &mut dyn FnMut() -> io::Result<(Vec<u8>, Vec<u8>)>,
    ) -> Option<Pins> {
        let mut verified = self.verify_pair(which, load())?;
        if matches!(
            verified,
            Err(PinsError::Signature(clauseo_blocklist::Error::Signature))
        ) {
            verified = self.verify_pair(which, load())?;
        }
        match verified {
            Ok(pins) => Some(pins),
            Err(error) => {
                tracing::warn!(which, %error, "a browser pins copy was refused");
                None
            }
        }
    }

    /// The check of one read pair; `None` when its files are missing or unreadable (logged).
    fn verify_pair(
        &self,
        which: &'static str,
        pair: io::Result<(Vec<u8>, Vec<u8>)>,
    ) -> Option<Result<Pins, PinsError>> {
        let (body, signature) = match pair {
            Ok(pair) => pair,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(error) => {
                tracing::warn!(which, kind = ?error.kind(), "a browser pins copy is unreadable");
                return None;
            }
        };
        let keys: Vec<&str> = self.keys.iter().map(String::as_str).collect();
        Some(Pins::verify(&body, &signature, &keys))
    }
}

/// The manifest and the signature in `folder`, each within its size cap.
fn read_pair(folder: &Path) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let body = read_capped(&folder.join(MANIFEST_FILE), MAX_MANIFEST)?;
    let signature = read_capped(
        &folder.join(SIGNATURE_FILE),
        u64::try_from(clauseo_blocklist::MAX_SIGNATURE).unwrap_or(u64::MAX),
    )?;
    Ok((body, signature))
}

/// Reads `path`, failing (`InvalidData`) when it holds more than `cap` bytes.
fn read_capped(path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(cap.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > cap {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "over the size cap",
        ));
    }
    Ok(bytes)
}

/// Stores `pins`, read from `body` and `signature`, in `folder` (created if missing), as the
/// block list's pair: the floor record raised to them first (`floor::raise`), then each file
/// replaced atomically, the manifest first and its signature last, then the folder synced. A
/// crash between the two leaves a pair that fails verification, and readers use no copy below
/// the floor until the next fetch writes a good pair.
pub(crate) fn store(folder: &Path, pins: &Pins, body: &[u8], signature: &[u8]) -> io::Result<()> {
    floor::raise(folder, pins)?;
    let manifest = folder.join(MANIFEST_FILE);
    let minisig = folder.join(SIGNATURE_FILE);
    write::sweep_temp_files(&manifest);
    write::sweep_temp_files(&minisig);
    write::replace_private_file(&manifest, body)?;
    write::replace_private_file(&minisig, signature)?;
    write::sync_dir(folder)
}

#[cfg(test)]
pub(crate) mod tests;
