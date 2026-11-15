//! The browser listener's credential, as the host hands it to the browser.

use std::fmt;

use crate::proxy::token::{self, Token};

/// A proxy credential for `Proxy-Authorization: Basic base64("<user>:<password>")`: the user
/// is always `clauseo`, the password 64 hex digits (32 random bytes). A secret: `Debug` shows
/// no part of the password, and there is no `Display`. Keep it out of logs and records.
#[derive(Clone)]
pub struct ProxyCredential {
    /// Always [`token::USER`].
    user: &'static str,
    password: String,
}

impl ProxyCredential {
    /// A credential with user `clauseo` and this password. The proxy makes its own
    /// ([`ProxyHandle::browser_credential`](crate::ProxyHandle::browser_credential)); this is
    /// for a stand-in proxy in tests.
    pub fn new(password: String) -> ProxyCredential {
        ProxyCredential {
            user: token::USER,
            password,
        }
    }

    pub(crate) fn of(token: &Token) -> ProxyCredential {
        ProxyCredential::new(token.hex().to_owned())
    }

    /// The user name: `clauseo`.
    pub fn user(&self) -> &str {
        self.user
    }

    /// The password: the token as 64 hex digits.
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for ProxyCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyCredential")
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_password() {
        let token = Token::from_bytes(&[0xab; 32]);
        let credential = ProxyCredential::of(&token);
        assert_eq!(credential.user(), "clauseo");
        assert_eq!(credential.password(), token.hex());
        let debug = format!("{credential:?}");
        assert!(!debug.contains("abab"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        let made = ProxyCredential::new("cafe".to_owned());
        assert_eq!((made.user(), made.password()), ("clauseo", "cafe"));
    }
}
