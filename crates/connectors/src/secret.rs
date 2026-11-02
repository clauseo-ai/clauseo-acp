//! [`Secret`]: a token, code, verifier or key, which never prints and is zeroed on drop.

use std::fmt;

use rand::RngCore as _;
use zeroize::Zeroize as _;

/// A secret string. `Debug` prints `Secret(..)`; there is no `Display` and no `Serialize`, so
/// a secret cannot reach a log line, an error or a file by accident. Its bytes are zeroed on
/// drop. Read it with [`Secret::expose`] only where it is sent or stored.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Takes ownership of `value` without copying it.
    pub fn new(value: String) -> Secret {
        Secret(value)
    }

    /// The secret's text, for the one place that sends or stores it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Secret {
        Secret(value.to_owned())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// An access token or API key, for an `Authorization: Bearer` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bearer(pub Secret);

/// 32 random bytes from the OS generator as 64 lowercase hex characters: the token R24's relay
/// hands its agents.
pub fn new_relay_token() -> Secret {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex(&bytes);
    bytes.zeroize();
    token
}

/// `bytes` as lowercase hex, built in place so no copy of it is left behind.
pub(crate) fn hex(bytes: &[u8]) -> Secret {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    Secret(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relay_token_is_64_hex_characters_and_new_each_time() {
        let one = new_relay_token();
        let two = new_relay_token();
        assert_eq!(one.len(), 64);
        assert!(
            one.expose()
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(one, two);
    }

    #[test]
    fn hex_spells_each_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]).expose(), "000fa5ff");
    }
}
