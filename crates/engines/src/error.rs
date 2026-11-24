//! Why a list was refused. None of these quotes the list's content beyond its `seq`.

use std::fmt;

/// Why a list was refused. The caller keeps what it has.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// `engines.json` is over [`MAX_JSON`](crate::MAX_JSON) bytes.
    TooBig,
    /// The `.sig` is not base64 of a minisign signature file, or is over
    /// [`MAX_SIGNATURE`](crate::MAX_SIGNATURE) bytes.
    SignatureFile,
    /// The key is not Tauri's form of a minisign public key (or none is compiled in).
    NoKey,
    /// The key does not verify the signature over these bytes: another key, a changed byte, a
    /// changed trusted comment, or a legacy (not prehashed) signature.
    Signature,
    /// The signed trusted comment is not exactly `timestamp:<digits>\tfile:engines.json`.
    Comment,
    /// Not UTF-8 JSON, not an object, a field missing or of the wrong type.
    Json,
    /// A top-level field that `clauseo-engines/1` does not have.
    UnknownField,
    /// `format` is not [`FORMAT`](crate::FORMAT).
    Format,
    /// `seq` is 0.
    Seq,
    /// `generated` is not `YYYY-MM-DDTHH:MM:SSZ` with each part in range.
    Generated,
    /// `apps` is empty, or an entry is empty.
    NoApps,
    /// A `sizes` row with an unknown agent, OS or architecture, or a zero size.
    Size,
    /// Two `sizes` rows for the same (agent, os, arch).
    DuplicateSize,
    /// `package_json` has a field an engines pin file doesn't (such as `scripts`), or isn't an
    /// object.
    PackageJsonField,
    /// `package_json.dependencies` keys are not exactly [`DEPENDENCIES`](crate::DEPENDENCIES).
    Dependencies,
    /// `package_json.overrides` has a key outside [`OVERRIDES`](crate::OVERRIDES), a value that
    /// isn't a string, or lacks an override the bundle pins.
    Overrides,
    /// `package_lock.lockfileVersion` is not 3, or `packages` is not an object.
    Lockfile,
    /// A `packages` entry (not the root) has no `integrity` starting `sha512-`.
    Integrity,
    /// A `packages` entry has a `resolved` that doesn't start with
    /// [`NPM_REGISTRY`](crate::NPM_REGISTRY).
    Resolved,
    /// The running app's version is not in `apps`.
    AppNotListed,
    /// `seq` is not above both the bundled seq and the seq in use (equal is not newer).
    NotNewer {
        /// The list's `seq`.
        seq: u64,
        /// The larger of the bundled seq and the seq in use.
        floor: u64,
    },
    /// A version in the list is not plain semver (a range or a tag, say).
    Version,
    /// A version in the bundled `package.json` is missing or not plain semver.
    BundledVersion,
    /// A version in the list is older than the bundled one.
    OlderThanBundled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::TooBig => "engines.json is over the size bound",
            Error::SignatureFile => "the signature file is not a Tauri updater signature",
            Error::NoKey => "the updater public key is not usable",
            Error::Signature => "the signature does not verify",
            Error::Comment => "the trusted comment is not timestamp:<n>\\tfile:engines.json",
            Error::Json => "engines.json is not the clauseo-engines/1 shape",
            Error::UnknownField => "engines.json has an unknown field",
            Error::Format => "engines.json is not clauseo-engines/1",
            Error::Seq => "the seq is 0",
            Error::Generated => "generated is not an RFC 3339 UTC time",
            Error::NoApps => "apps is empty or has an empty entry",
            Error::Size => "a sizes row is not valid",
            Error::DuplicateSize => "two sizes rows name the same agent, OS and architecture",
            Error::PackageJsonField => "package_json has a field a pin file doesn't",
            Error::Dependencies => "package_json's dependencies are not the two engines",
            Error::Overrides => "package_json's overrides are not the allowed ones",
            Error::Lockfile => "package_lock is not a version 3 lock",
            Error::Integrity => "a locked package has no sha512 integrity",
            Error::Resolved => "a locked package resolves outside the npm registry",
            Error::AppNotListed => "this app version is not in apps",
            Error::NotNewer { seq, floor } => {
                return write!(f, "the list's seq {seq} is not above {floor}");
            }
            Error::Version => "a listed version is not plain semver",
            Error::BundledVersion => "a bundled version is missing or not plain semver",
            Error::OlderThanBundled => "a listed version is older than the bundled one",
        };
        f.write_str(text)
    }
}

impl std::error::Error for Error {}
