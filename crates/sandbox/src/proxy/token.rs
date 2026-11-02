//! The agent listener's token: 32 random bytes per launch, as hex, sent as
//! `Proxy-Authorization: Basic base64("clauseo:<hex>")`.

use std::fmt;
use std::io;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use subtle::ConstantTimeEq;

use crate::Error;

/// The user name in the credential.
pub(crate) const USER: &str = "clauseo";

/// One launch's token. Never logged; `Debug` shows no part of it.
#[derive(Clone)]
pub(crate) struct Token {
    hex: String,
    /// `clauseo:<hex>`, the decoded credential a request must carry.
    credential: Vec<u8>,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl Token {
    /// A fresh token from the OS's randomness.
    pub(crate) fn generate() -> Result<Token, Error> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|error| Error::ProxyStart(io::Error::other(error.to_string())))?;
        Ok(Token::from_bytes(&bytes))
    }

    pub(crate) fn from_bytes(bytes: &[u8; 32]) -> Token {
        let digit = |nibble: u8| char::from_digit(u32::from(nibble), 16).unwrap_or('0');
        let hex: String = bytes
            .iter()
            .flat_map(|&b| [digit(b >> 4), digit(b & 0xf)])
            .collect();
        let credential = format!("{USER}:{hex}").into_bytes();
        Token { hex, credential }
    }

    /// The token as hex, for the env's proxy URL and the example's output.
    pub(crate) fn hex(&self) -> &str {
        &self.hex
    }

    /// True when the request's `Proxy-Authorization` values are exactly one `Basic` credential
    /// equal to this token's, compared in constant time over the whole decoded `user:token`.
    pub(crate) fn admits(&self, values: &[&[u8]]) -> bool {
        let [value] = values else {
            return false;
        };
        let Some(space) = value.iter().position(|&b| b == b' ') else {
            return false;
        };
        let (scheme, rest) = value.split_at(space);
        if !scheme.eq_ignore_ascii_case(b"basic") {
            return false;
        }
        let Ok(decoded) = STANDARD.decode(rest.trim_ascii()) else {
            return false;
        };
        decoded.ct_eq(&self.credential).into()
    }

    /// The header value a client sends, for tests.
    #[cfg(test)]
    pub(crate) fn header_value(&self) -> String {
        format!("Basic {}", STANDARD.encode(&self.credential))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_only_its_own_credential() {
        let token = Token::from_bytes(&[7; 32]);
        let other = Token::from_bytes(&[8; 32]);
        let good = token.header_value();
        assert!(token.admits(&[good.as_bytes()]));
        let lower = good.replacen("Basic", "basic", 1);
        assert!(token.admits(&[lower.as_bytes()]));
        assert!(!token.admits(&[]));
        assert!(!token.admits(&[good.as_bytes(), good.as_bytes()]));
        assert!(!token.admits(&[other.header_value().as_bytes()]));
        let wrong_user = format!("Basic {}", STANDARD.encode(format!("bob:{}", token.hex())));
        assert!(!token.admits(&[wrong_user.as_bytes()]));
        let bearer = good.replacen("Basic", "Bearer", 1);
        assert!(!token.admits(&[bearer.as_bytes()]));
        let short = format!("Basic {}", STANDARD.encode(&token.credential[..10]));
        assert!(!token.admits(&[short.as_bytes()]));
        assert!(!token.admits(&[b"Basic !!!"]));
    }

    #[test]
    fn hex_is_64_chars_and_debug_hides_it() {
        let token = Token::generate().unwrap();
        assert_eq!(token.hex().len(), 64);
        assert!(token.hex().bytes().all(|b| b.is_ascii_hexdigit()));
        let debug = format!("{token:?}");
        assert!(!debug.contains(token.hex()), "{debug}");
        assert_ne!(Token::generate().unwrap().hex(), token.hex());
    }
}
