//! Validated short-string ids, each a `CompactString` serialised as a plain string.

use std::fmt;
use std::str::FromStr;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::hex_value;
use crate::Error;
#[cfg(doc)]
use crate::FileRefId;

/// Defines a validated newtype over [`CompactString`], serialised as a plain string.
macro_rules! checked_string {
    ($(#[$meta:meta])* $name:ident, $check:path) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
                #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
        pub struct $name(CompactString);

        impl $name {
            /// Checks `value` against the rule above and wraps it.
            pub fn new(value: &str) -> Result<Self, Error> {
                $check(value).map_err(|rule| Error::Format { kind: stringify!($name), rule })?;
                Ok(Self(CompactString::from(value)))
            }

            /// The text inside.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(text: &str) -> Result<Self, Error> {
                Self::new(text)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = CompactString::deserialize(deserializer)?;
                $check(&value).map_err(|rule| {
                    de::Error::custom(Error::Format { kind: stringify!($name), rule })
                })?;
                Ok(Self(value))
            }
        }
    };
}

fn check_agent_id(value: &str) -> Result<(), &'static str> {
    let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
    if (1..=64).contains(&value.len()) && value.bytes().all(allowed) {
        Ok(())
    } else {
        Err("expected 1 to 64 characters of [a-z0-9-]")
    }
}

fn check_content_id(value: &str) -> Result<(), &'static str> {
    let allowed = |byte: u8| hex_value(byte).is_some();
    if (1..=64).contains(&value.len()) && value.bytes().all(allowed) {
        Ok(())
    } else {
        Err("expected 1 to 64 lowercase hex characters")
    }
}

fn check_agent_session_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        Err("must not be empty")
    } else {
        Ok(())
    }
}

/// The most bytes in one segment of a [`MatterPath`]: the common file-name limit (APFS, NTFS,
/// ext4).
pub(crate) const MAX_SEGMENT_BYTES: usize = 255;
/// The most bytes in a whole [`MatterPath`].
pub(crate) const MAX_PATH_BYTES: usize = 4096;

fn check_matter_path(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("must not be empty");
    }
    if value.len() > MAX_PATH_BYTES {
        return Err("longer than 4096 bytes");
    }
    if value.contains('\\') {
        return Err("must not contain a backslash");
    }
    if value.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
        return Err("must not contain a control character");
    }
    // A drive form: `C:`, `C:x`, `C:/...`.
    if let [first, b':', ..] = value.as_bytes()
        && first.is_ascii_alphabetic()
    {
        return Err("must not start with a drive letter and `:`");
    }
    for segment in value.split('/') {
        // A leading `/` makes an empty first segment, a trailing one an empty last segment.
        if matches!(segment, "" | "." | "..") {
            return Err("must be relative, with no empty, `.` or `..` segment");
        }
        if segment.len() > MAX_SEGMENT_BYTES {
            return Err("has a segment longer than 255 bytes");
        }
    }
    Ok(())
}

checked_string!(
    /// A registry id for an agent (`claude-acp`, `codex-acp`; `fake-agent` in tests): 1 to 64
    /// characters of `[a-z0-9-]`.
    AgentId, check_agent_id
);
checked_string!(
    /// The ACP session id the agent gave. A foreign attribute, never a primary key. Opaque and
    /// non-empty.
    AgentSessionId, check_agent_session_id
);
checked_string!(
    /// An opaque id of a file's content (`matters` fills it from git's blob id): 1 to 64 lowercase
    /// hex characters. Clients only compare two for equality.
    ContentId, check_content_id
);
checked_string!(
    /// A path inside a matter: `/` separators, no leading `/`, no `\`, no control character
    /// (NUL to 0x1F and DEL), no empty, `.` or `..` segment, no drive form (`C:`, `C:x`,
    /// `C:/..`), at most 255 bytes per segment and 4096 bytes in all. Never an id: commands name
    /// files by [`FileRefId`].
    ///
    /// A `:` elsewhere is allowed, because macOS file names may hold one. On Windows a `:` names an
    /// alternate data stream, so `matters` must refuse a path containing `:` there.
    MatterPath, check_matter_path
);
