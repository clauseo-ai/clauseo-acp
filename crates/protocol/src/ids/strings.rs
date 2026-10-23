//! Validated short-string ids, each a `CompactString` serialised as a plain string.

use std::fmt;
use std::str::FromStr;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::hex_value;
use crate::Error;
#[cfg(doc)]
use crate::FileRefId;

/// Defines a validated newtype over [`CompactString`], serialised as a plain string. The struct
/// names its check, a function that returns the broken rule as a fixed sentence.
macro_rules! checked_string {
    ($(#[doc = $doc:expr])* pub struct $name:ident($check:ident);) => {
        $(#[doc = $doc])*
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

/// The most characters in an [`AgentId`].
const MAX_AGENT_ID_CHARS: usize = 64;

fn check_agent_id(value: &str) -> Result<(), &'static str> {
    let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
    if (1..=MAX_AGENT_ID_CHARS).contains(&value.len()) && value.bytes().all(allowed) {
        Ok(())
    } else {
        Err("expected 1 to 64 characters of [a-z0-9-]")
    }
}

/// The most characters in a [`ContentId`].
const MAX_CONTENT_ID_CHARS: usize = 64;

fn check_content_id(value: &str) -> Result<(), &'static str> {
    let allowed = |byte: u8| hex_value(byte).is_some();
    if (1..=MAX_CONTENT_ID_CHARS).contains(&value.len()) && value.bytes().all(allowed) {
        Ok(())
    } else {
        Err("expected 1 to 64 lowercase hex characters")
    }
}

/// Most UTF-8 bytes in an [`AgentChoiceId`]. ACP's ids are short words; 256 leaves room without
/// letting a client send megabytes.
pub const MAX_ID_TEXT_BYTES: usize = 256;

fn check_agent_choice_id(value: &str) -> Result<(), &'static str> {
    if (1..=MAX_ID_TEXT_BYTES).contains(&value.len()) {
        Ok(())
    } else {
        Err("expected 1 to 256 UTF-8 bytes")
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
    if value.chars().any(char::is_control) {
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

checked_string! {
    /// A registry id for an agent (`claude-acp`, `codex-acp`; `fake-agent` in tests): 1 to 64
    /// characters of `[a-z0-9-]`.
    pub struct AgentId(check_agent_id);
}
checked_string! {
    /// An id the agent chose and a command names back: a permission option's id, a config
    /// option's id, a select choice's value, a mode's id. 1 to [`MAX_ID_TEXT_BYTES`] UTF-8 bytes,
    /// otherwise opaque. Commands only: events keep these ids as the agent's plain text, so an
    /// agent's long or empty id never makes the log fail to decode. In return `acp` refuses an
    /// offered permission option, mode or config option (or select value) whose id fails
    /// [`AgentChoiceId::new`], failing that request with `agent.protocol_error`, so the UI never
    /// shows an offer no command can answer.
    pub struct AgentChoiceId(check_agent_choice_id);
}
checked_string! {
    /// The ACP session id the agent gave. A foreign attribute, never a primary key. Opaque and
    /// non-empty, with no length cap on purpose: it is the agent's text, kept as given so a
    /// session with a long id still attaches, and a 1 MiB id decodes.
    pub struct AgentSessionId(check_agent_session_id);
}
checked_string! {
    /// An opaque id of a file's content (`matters` fills it from git's blob id): 1 to 64 lowercase
    /// hex characters. Clients only compare two for equality.
    pub struct ContentId(check_content_id);
}
checked_string! {
    /// A path inside a matter: `/` separators, no leading `/`, no `\`, no Unicode control
    /// character (`char::is_control`: C0, DEL and C1), no empty, `.` or `..` segment, no drive
    /// form (`C:`, `C:x`, `C:/..`), at most 255 bytes per segment and 4096 bytes in all. Never an
    /// id: commands name files by [`FileRefId`].
    ///
    /// Bidi marks and other format characters are allowed (Hebrew and Arabic file names hold
    /// U+200F), so the UI shows a path bidi-isolated. A `:` elsewhere is allowed, because macOS
    /// file names may hold one. On Windows `matters` must refuse a path with a `:` (an alternate
    /// data stream), a part for which [`is_windows_reserved_name`] is true, and a part ending in
    /// a dot or a space.
    ///
    /// Two paths are the same path only when their bytes are equal: no case folding and no
    /// Unicode normalisation, although APFS and NTFS ignore case and APFS ignores normalisation,
    /// so `café.docx` in NFC and in NFD are two paths here. `matters` and the UI compare paths
    /// the same way, and the host matches an `UndoStep`'s paths against the step's
    /// `FilesChanged` exactly, so a path the file system would read as another file is refused.
    pub struct MatterPath(check_matter_path);
}

/// Windows device names, matched against a name's stem with `eq_ignore_ascii_case`, so the
/// superscript digits are listed as they are.
const WINDOWS_RESERVED_NAMES: [&str; 32] = [
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Whether Windows reads a file or folder name as a device: CON, PRN, AUX, NUL, CONIN$,
/// CONOUT$, COM0 to COM9, COM¹ to COM³, LPT0 to LPT9 or LPT¹ to LPT³, in any case, with or
/// without an extension. The stem (the text before the first `.`) is matched after its
/// trailing spaces are trimmed, as Windows does, so `"CON .txt"`, `"nul .x"` and `"con."` are
/// device names and `"CONSOLE"` and `"com10"` are not. Used for a new matter folder's name and
/// by `matters` for each part of a [`MatterPath`] on Windows.
pub fn is_windows_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
}

/// Each rule message copies its limit as a number, so a test ties the two: one past the limit
/// fails with a message that holds the limit, and the limit itself passes.
#[cfg(test)]
mod rule_numbers {
    use super::{
        MAX_AGENT_ID_CHARS, MAX_CONTENT_ID_CHARS, MAX_ID_TEXT_BYTES, MAX_PATH_BYTES,
        MAX_SEGMENT_BYTES, check_agent_choice_id, check_agent_id, check_content_id,
        check_matter_path,
    };

    type Check = fn(&str) -> Result<(), &'static str>;

    fn holds_its_limit(check: Check, limit: usize, at: &dyn Fn(usize) -> String) {
        assert_eq!(check(&at(limit)), Ok(()), "at {limit}");
        let rule = check(&at(limit + 1)).unwrap_err();
        assert!(
            rule.contains(&limit.to_string()),
            "{rule:?} does not name {limit}"
        );
    }

    #[test]
    fn each_rule_message_holds_its_constant() {
        holds_its_limit(check_agent_id, MAX_AGENT_ID_CHARS, &|n| "a".repeat(n));
        holds_its_limit(check_content_id, MAX_CONTENT_ID_CHARS, &|n| "a".repeat(n));
        holds_its_limit(check_agent_choice_id, MAX_ID_TEXT_BYTES, &|n| "a".repeat(n));
        // `a/a/a...` cut to `n` bytes, ending in `aa` rather than a `/`.
        let path = |n: usize| {
            let mut path = "a/".repeat(n);
            path.truncate(n);
            if path.ends_with('/') {
                path.pop();
                path.push('a');
            }
            path
        };
        holds_its_limit(check_matter_path, MAX_PATH_BYTES, &path);
        holds_its_limit(check_matter_path, MAX_SEGMENT_BYTES, &|n| "a".repeat(n));
    }
}
