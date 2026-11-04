//! The ids, the version and the error code: the closed types that travel as one fixed string.

use std::fmt;
use std::str::FromStr;

use clauseo_protocol::ErrorCode;
use uuid::Uuid;

use super::text::as_text;
use super::{Os, OsMajor};

/// The pattern of a UUID v4 on the wire: lowercase, hyphenated, version 4, RFC 4122 variant.
pub const UUID_V4_PATTERN: &str =
    "^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$";

/// A string that is not an id, version or code in the one form the schema allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadText;

impl fmt::Display for BadText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a value in the schema's form")
    }
}

impl std::error::Error for BadText {}

impl From<uuid::Error> for BadText {
    fn from(_: uuid::Error) -> BadText {
        BadText
    }
}

impl From<clauseo_protocol::Error> for BadText {
    fn from(_: clauseo_protocol::Error) -> BadText {
        BadText
    }
}

/// A random UUID v4, read only in its lowercase hyphenated form.
fn parse_v4(text: &str) -> Result<Uuid, BadText> {
    let uuid = Uuid::try_parse(text)?;
    let canonical = uuid.get_version_num() == 4
        && uuid.get_variant() == uuid::Variant::RFC4122
        && uuid.hyphenated().encode_lower(&mut Uuid::encode_buffer()) == text;
    canonical.then_some(uuid).ok_or(BadText)
}

macro_rules! v4_id {
    ($($doc:literal)* $name:ident, $schema_name:literal) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// A new id from the OS's random source.
            #[cfg(feature = "client")]
            pub fn random() -> $name {
                $name(Uuid::new_v4())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0.hyphenated(), f)
            }
        }

        impl FromStr for $name {
            type Err = BadText;

            /// Exactly a lowercase, hyphenated UUID v4.
            fn from_str(text: &str) -> Result<$name, BadText> {
                parse_v4(text).map($name)
            }
        }

        as_text!(
            $name,
            $schema_name,
            schema: schemars::json_schema!({
                "type": "string",
                "format": "uuid",
                "pattern": UUID_V4_PATTERN,
            })
        );

        #[cfg(test)]
        impl super::tests::sample::Sample for $name {
            fn sample(tape: &mut super::tests::sample::Tape) -> Self {
                let bits = u128::from(tape.next()) << 64 | u128::from(tape.next());
                $name(uuid::Builder::from_random_bytes(bits.to_le_bytes()).into_uuid())
            }
        }
    };
}

v4_id! {
    "The install id (research 37 §3): a random UUID v4, made at the first act, deleted when"
    "counts turn off and made anew when they turn on. It travels only in the day report."
    InstallId, "InstallId"
}

v4_id! {
    "A weekly tally's own random id, made when its week closes and reused only on retries, so"
    "the Worker can take a tally once. It is never the install id."
    TallyId, "TallyId"
}

/// A released version, `major.minor.patch`, each part 0 to 999 with no leading zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

/// The pattern of a version on the wire.
pub const VERSION_PATTERN: &str =
    "^(0|[1-9][0-9]{0,2})\\.(0|[1-9][0-9]{0,2})\\.(0|[1-9][0-9]{0,2})$";

impl Version {
    /// The largest part.
    const MAX_PART: u16 = 999;

    /// The version, or `None` when a part is over 999.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Option<Version> {
        if major > Self::MAX_PART || minor > Self::MAX_PART || patch > Self::MAX_PART {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = BadText;

    /// Exactly `major.minor.patch` (desktop's `CARGO_PKG_VERSION` for a release); no pre-release
    /// or build text.
    fn from_str(text: &str) -> Result<Version, BadText> {
        let mut parts = text.split('.').map(|part| {
            let digits =
                !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit());
            let no_leading_zero = part == "0" || !part.starts_with('0');
            (digits && no_leading_zero)
                .then(|| part.parse::<u16>().ok())
                .flatten()
        });
        let (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(BadText);
        };
        Version::new(major, minor, patch).ok_or(BadText)
    }
}

as_text!(Version, "Version", VERSION_PATTERN);

#[cfg(test)]
impl super::tests::sample::Sample for Version {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        let mut part = || u16::try_from(tape.below(1000)).unwrap();
        Version::new(part(), part(), part()).unwrap()
    }
}

/// An [`ErrorCode`] this build knows, never `unknown`. `ErrorCode`'s own `Deserialize` turns a
/// string it does not know into `Unknown`, which the schema must refuse instead; this one reads
/// only through `ErrorCode::from_str` and refuses `unknown` too. Ordered by its name, so cells
/// sort the same on every build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrictErrorCode(ErrorCode);

impl Ord for StrictErrorCode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_str().cmp(other.0.as_str())
    }
}

impl PartialOrd for StrictErrorCode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl StrictErrorCode {
    /// The code, or `None` for [`ErrorCode::Unknown`].
    pub fn new(code: ErrorCode) -> Option<StrictErrorCode> {
        (code != ErrorCode::Unknown).then_some(StrictErrorCode(code))
    }

    /// The code.
    pub const fn get(self) -> ErrorCode {
        self.0
    }

    /// Every code that may travel: [`ErrorCode::ALL`] without `unknown`.
    pub fn all() -> impl Iterator<Item = StrictErrorCode> {
        ErrorCode::ALL.into_iter().filter_map(StrictErrorCode::new)
    }
}

impl fmt::Display for StrictErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl FromStr for StrictErrorCode {
    type Err = BadText;

    fn from_str(text: &str) -> Result<StrictErrorCode, BadText> {
        let code = ErrorCode::from_str(text)?;
        StrictErrorCode::new(code).ok_or(BadText)
    }
}

as_text!(
    StrictErrorCode,
    "StrictErrorCode",
    schema: {
        let codes: Vec<&str> = StrictErrorCode::all().map(|code| code.0.as_str()).collect();
        schemars::json_schema!({ "type": "string", "enum": codes })
    }
);

#[cfg(test)]
impl super::tests::sample::Sample for StrictErrorCode {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        let codes: Vec<StrictErrorCode> = StrictErrorCode::all().collect();
        tape.pick(&codes)
    }
}

/// The last macOS major version the schema takes. macOS is numbered by year since 26 (2025), so
/// 40 is the 2039 release: headroom for any build sent today, and a schema change before then.
pub const MACOS_LAST: u8 = 40;

/// The OS and its major version, as only the schema's pairs: macOS 13 to 40, Windows 10 or 11,
/// Linux with none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    /// macOS, major version 13 to 40.
    Macos(OsMajor),
    /// Windows 10 or 11.
    Windows(OsMajor),
    /// Linux.
    Linux,
}

impl Platform {
    /// macOS `major`, or `None` outside 13 to 40.
    pub fn macos(major: u8) -> Option<Platform> {
        Platform::from_parts(Os::Macos, OsMajor::new(major))
    }

    /// Windows `major`, or `None` unless it is 10 or 11.
    pub fn windows(major: u8) -> Option<Platform> {
        Platform::from_parts(Os::Windows, OsMajor::new(major))
    }

    /// The pair, or `None` when the schema does not allow it.
    pub fn from_parts(os: Os, major: Option<OsMajor>) -> Option<Platform> {
        let number = major.map(OsMajor::get);
        match (os, number) {
            (Os::Macos, Some(13..=MACOS_LAST)) => major.map(Platform::Macos),
            (Os::Windows, Some(10 | 11)) => major.map(Platform::Windows),
            (Os::Linux, None) => Some(Platform::Linux),
            _ => None,
        }
    }

    /// `os` on the wire.
    pub const fn os(self) -> Os {
        match self {
            Platform::Macos(_) => Os::Macos,
            Platform::Windows(_) => Os::Windows,
            Platform::Linux => Os::Linux,
        }
    }

    /// `os_major` on the wire, which Linux leaves out.
    pub const fn os_major(self) -> Option<OsMajor> {
        match self {
            Platform::Macos(major) | Platform::Windows(major) => Some(major),
            Platform::Linux => None,
        }
    }
}

#[cfg(test)]
impl super::tests::sample::Sample for Platform {
    fn sample(tape: &mut super::tests::sample::Tape) -> Self {
        let all: Vec<Platform> = (13..=MACOS_LAST)
            .filter_map(Platform::macos)
            .chain([10, 11].into_iter().filter_map(Platform::windows))
            .chain([Platform::Linux])
            .collect();
        tape.pick(&all)
    }
}
