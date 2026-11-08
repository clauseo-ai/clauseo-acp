//! The update's file binding: an update installs only the file its signature names.
//!
//! The plugin verifies an artifact's signature against the updater key, but not the trusted
//! comment that signature carries beyond its `version:`. A release signs several files with one
//! key at one version: each Windows architecture's installer, and the sandbox tool
//! (`clauseo-sandbox-setup.exe`, release.yml's `SANDBOX_EXE`). A tampered `latest.json` could pair
//! any of them with this platform, and the plugin would install it. So before the download is
//! saved and again before it is installed, [`check_update_file`] verifies the whole signature
//! (the file's, and minisign's global signature over it and the trusted comment) with the same
//! key, and only then reads the trusted comment, whose `file:` must be this platform's installer
//! ([`update_file_name`]) and whose `version:` the update's version.
//!
//! `tauri signer sign --app-version <v>` writes the trusted comment as tab-separated fields,
//! `timestamp:<unix>\tfile:<base name of the signed file>\tversion:<v>` (tauri-cli 2.12.2,
//! `helpers/updater_signature.rs`), which release.yml's own check of each Windows installer
//! expects too (`latest-json.mjs --verify`, `signatureCheck`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clauseo_host::UpdaterError;
use minisign_verify::{PublicKey, Signature};

use super::arch::Target;
use super::install::LIFECYCLE;

/// The product name in an installer's name: `tauri.conf.json`'s `productName`, which Tauri's
/// NSIS bundle puts first (`<productName>_<version>_<arch>-setup.exe`).
const PRODUCT: &str = "Clauseo";

/// The macOS updater archive's name as the bundler signs it: both macOS legs bundle
/// `bundle/macos/Clauseo.app.tar.gz` and sign it under that base name (release.yml, "Collect
/// this leg's assets"); the release renames it per architecture only after signing ("Name the
/// assets"), so the signed name cannot tell x64 from aarch64, and the version binds it.
const MAC_ARCHIVE: &str = "Clauseo.app.tar.gz";

/// How the release names the macOS archive it signs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MacName {
    /// The bundler's `Clauseo.app.tar.gz`, one name for both architectures (today's release).
    Bundle,
    /// `Clauseo_<version>_<aarch64|x64>.app.tar.gz`, the asset's name, once the release signs
    /// after renaming (stage F).
    PerArch,
}

/// The macOS name in force. Flip [`MAC_SIGNS_PER_ARCH`] only together with the release change
/// that signs the renamed archive: an app bound to the per-arch name refuses every update the
/// release signs as `Clauseo.app.tar.gz`.
pub(super) const MAC_NAME: MacName = if MAC_SIGNS_PER_ARCH {
    MacName::PerArch
} else {
    MacName::Bundle
};

/// The switch [`MAC_NAME`] follows: whether the release signs the macOS archive under its
/// per-arch asset name. `false` until stage F's release signs after renaming.
const MAC_SIGNS_PER_ARCH: bool = false;

/// Why an update's file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FileBindingError {
    /// This platform installs no update, so no file is expected.
    NoPlatform,
    /// The key or the signature is not base64 of minisign's text.
    Unreadable,
    /// The signature does not verify over the bytes, or its global signature does not cover the
    /// trusted comment (a changed comment).
    Unverified,
    /// The verified trusted comment holds no single `file:` field.
    NoFile,
    /// The verified trusted comment names another file.
    WrongFile {
        /// The `file:` it names.
        found: String,
    },
    /// The verified trusted comment holds no single `version:` field.
    NoVersion,
    /// The verified trusted comment names another version.
    WrongVersion {
        /// The `version:` it names.
        found: String,
    },
}

/// The architecture name Clauseo's Windows installers carry, for a Rust `target_arch`: release.yml
/// names each one `Clauseo_<version>_<matrix.arch>-setup.exe`, `x64` for `x86_64-pc-windows-msvc`
/// and `arm64` for `aarch64-pc-windows-msvc` (Tauri's NSIS names).
pub(super) fn installer_arch(target_arch: &str) -> Option<&'static str> {
    match target_arch {
        "x86_64" => Some("x64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

/// The architecture name the release puts in a macOS archive's asset name (release.yml, "Name
/// the assets"): `aarch64` for `aarch64-apple-darwin`, `x64` for `x86_64-apple-darwin`.
pub(super) fn mac_archive_arch(target_arch: &str) -> Option<&'static str> {
    match target_arch {
        "x86_64" => Some("x64"),
        "aarch64" => Some("aarch64"),
        _ => None,
    }
}

/// The file an update of `version` must have been signed as on `os` (`std::env::consts::OS`)
/// and `target_arch`, with macOS named as `mac`: `Clauseo_<version>_<x64|arm64>-setup.exe` on
/// Windows, `Clauseo.app.tar.gz` or `Clauseo_<version>_<aarch64|x64>.app.tar.gz` on macOS, and
/// none elsewhere (Linux updates through its package manager).
pub(super) fn update_file_name(
    mac: MacName,
    os: &str,
    target_arch: &str,
    version: &str,
) -> Option<String> {
    match (os, mac) {
        ("windows", _) => {
            installer_arch(target_arch).map(|arch| format!("{PRODUCT}_{version}_{arch}-setup.exe"))
        }
        ("macos", MacName::Bundle) => Some(MAC_ARCHIVE.to_owned()),
        ("macos", MacName::PerArch) => mac_archive_arch(target_arch)
            .map(|arch| format!("{PRODUCT}_{version}_{arch}.app.tar.gz")),
        _ => None,
    }
}

/// [`update_file_name`] for `target`'s platform, the installer the check that found the update
/// asked for (`arch.rs`): this build's own, or Windows on arm64. The macOS name in force is
/// [`MAC_NAME`].
pub(super) fn expected_update_file(version: &str, target: Target) -> Option<String> {
    let (os, arch) = target.platform();
    update_file_name(MAC_NAME, os, arch, version)
}

/// Checks that `signature` (Tauri's form: base64 of minisign's `.sig` text) signs `bytes` under
/// `pubkey` (base64 of minisign's `.pub` text), global signature included, and that its trusted
/// comment then names exactly `expected_file` and `expected_version`. Legacy (`Ed`) signatures
/// verify as the plugin's own check lets them; Tauri signs prehashed (`ED`).
pub(super) fn check_update_file(
    pubkey: &str,
    signature: &str,
    bytes: &[u8],
    expected_file: &str,
    expected_version: &str,
) -> Result<(), FileBindingError> {
    let key = decode_text(pubkey)
        .and_then(|text| PublicKey::decode(&text).ok())
        .ok_or(FileBindingError::Unreadable)?;
    let signature = decode_text(signature)
        .and_then(|text| Signature::decode(&text).ok())
        .ok_or(FileBindingError::Unreadable)?;
    key.verify(bytes, &signature, true)
        .map_err(|_unverified| FileBindingError::Unverified)?;
    // Only now is the trusted comment trusted: the global signature above covers it.
    let comment = signature.trusted_comment();
    let file = field(comment, "file").ok_or(FileBindingError::NoFile)?;
    if file != expected_file {
        return Err(FileBindingError::WrongFile {
            found: file.to_owned(),
        });
    }
    let version = field(comment, "version").ok_or(FileBindingError::NoVersion)?;
    if version != expected_version {
        return Err(FileBindingError::WrongVersion {
            found: version.to_owned(),
        });
    }
    Ok(())
}

/// Base64 `text` (surrounding whitespace allowed), decoded as UTF-8.
fn decode_text(text: &str) -> Option<String> {
    let bytes = STANDARD.decode(text.trim()).ok()?;
    String::from_utf8(bytes).ok()
}

/// The value of the trusted comment's one `<name>:` field; none when it has none or more than
/// one, which `tauri signer` never writes.
pub(super) fn field<'a>(comment: &'a str, name: &str) -> Option<&'a str> {
    let mut values = comment
        .split('\t')
        .filter_map(|part| part.strip_prefix(name)?.strip_prefix(':'));
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

/// The adapter's gate before a download is saved and before it is installed: refuses `bytes`
/// unless `signature` signs them under `pubkey` (none: no key is set) as this platform's file of
/// `version` for `target`. A refusal is logged on the lifecycle target with the expected and
/// found names (never the signature) and is the host's signature failure, so the update fails
/// as any bad signature does.
pub(super) fn bind(
    pubkey: Option<&str>,
    signature: &str,
    version: &str,
    bytes: &[u8],
    target: Target,
) -> Result<(), UpdaterError> {
    let expected = expected_update_file(version, target);
    let checked = match (&expected, pubkey) {
        (None, _) => Err(FileBindingError::NoPlatform),
        (Some(_), None) => Err(FileBindingError::Unreadable),
        (Some(file), Some(key)) => check_update_file(key, signature, bytes, file, version),
    };
    let Err(error) = checked else {
        return Ok(());
    };
    let expected = expected.as_deref().unwrap_or("none");
    // What the trusted comment named, once verified: a field read before the global signature
    // held is never logged.
    let (found_file, found_version) = match &error {
        FileBindingError::WrongFile { found } => (found.as_str(), "unchecked"),
        FileBindingError::NoFile => ("missing", "unchecked"),
        FileBindingError::WrongVersion { found } => (expected, found.as_str()),
        FileBindingError::NoVersion => (expected, "missing"),
        FileBindingError::NoPlatform
        | FileBindingError::Unreadable
        | FileBindingError::Unverified => ("unverified", "unverified"),
    };
    let reason = line(&error);
    tracing::warn!(
        target: LIFECYCLE,
        expected_file = expected,
        expected_version = version,
        found_file,
        found_version,
        reason,
        "the update's signature does not name this platform's installer: refused"
    );
    Err(UpdaterError::Signature(reason.to_owned()))
}

/// The host's fixed line for `error`: no name, path or signature.
pub(super) fn line(error: &FileBindingError) -> &'static str {
    match error {
        FileBindingError::NoPlatform => "this platform installs no update",
        FileBindingError::Unreadable => "the signature or the key is not readable",
        FileBindingError::Unverified => "the signature does not verify",
        FileBindingError::NoFile | FileBindingError::WrongFile { .. } => {
            "the signature is for another file than this platform's installer"
        }
        FileBindingError::NoVersion | FileBindingError::WrongVersion { .. } => {
            "the signature is for another version than the one offered"
        }
    }
}
