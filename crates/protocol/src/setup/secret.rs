//! [`Secret`]: a pasted sign-in code or an API key, which decodes from a window and never
//! encodes.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

/// The most UTF-8 bytes a [`Secret`] holds.
pub const MAX_SECRET_BYTES: usize = 4096;

/// A pasted sign-in code or an API key, as a window sends it: 1 to [`MAX_SECRET_BYTES`] bytes.
///
/// It decodes but never encodes: there is no `Serialize`, so nothing that holds one can be
/// written to a log, an event or a file through serde. `Debug` and `Display` print `<secret>`,
/// there is no `Clone`, and the text is zeroed when dropped. A decode error never quotes it.
/// TypeScript sees a plain `string`.
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = String))]
pub struct Secret(Zeroizing<String>);

impl Secret {
    /// Wraps `text`, which must be 1 to [`MAX_SECRET_BYTES`] bytes; `None` otherwise (the text
    /// is zeroed then too).
    pub fn new(text: Zeroizing<String>) -> Option<Self> {
        (1..=MAX_SECRET_BYTES)
            .contains(&text.len())
            .then(|| Self(text))
    }

    /// The text, for the one place that uses it: the keychain or the login's stdin.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The text, still zeroed when dropped.
    pub fn into_zeroizing(self) -> Zeroizing<String> {
        self.0
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        *self.0 == *other.0
    }
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<secret>")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = Zeroizing::new(String::deserialize(deserializer)?);
        Self::new(text).ok_or_else(|| {
            D::Error::custom(format_args!(
                "a secret must be 1 to {MAX_SECRET_BYTES} bytes"
            ))
        })
    }
}
