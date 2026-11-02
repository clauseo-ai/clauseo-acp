//! The credential in the keychain item, as a small JSON:
//! `{"v":1,"kind":"oauth","access_token":"...","access_expires_at":<unix s>,"refresh_token":"..."}`
//! or `{"v":1,"kind":"api_key","key":"cdk_live_..."}`. [`Credential::encode`] refuses anything
//! over [`MAX_SECRET_BYTES`]; [`Credential::decode`] reads an item that does not parse as
//! absent, and the caller deletes it.

use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroizing;

use crate::error::StoreError;
use crate::secret::Secret;
use crate::store::{MAX_SECRET_BYTES, check_size};

/// The credential format's version.
const VERSION: u32 = 1;

/// How a connector is signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CredentialKind {
    /// OAuth tokens from the browser sign-in.
    #[serde(rename = "oauth")]
    OAuth,
    /// A firm's pasted `cdk_live_` key.
    #[serde(rename = "api_key")]
    ApiKey,
}

/// What the app shows for a connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialStatus {
    /// Never added on this install.
    NotAdded,
    /// Added, with a credential in the keychain.
    SignedIn(CredentialKind),
    /// Added, but signed out (by the lawyer, or a refused refresh).
    SignedOut,
    /// Added with an API key the connector refused.
    KeyRevoked,
}

/// What `sign_out` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignOutReport {
    /// Whether the connector confirmed the revocation. The keychain item is deleted either way.
    pub revoked: bool,
}

/// A connector's credential, in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Credential {
    /// OAuth tokens.
    OAuth(OAuthTokens),
    /// An API key.
    ApiKey(Secret),
}

/// An access token, when it expires, and the refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OAuthTokens {
    pub(crate) access_token: Secret,
    /// Unix seconds.
    pub(crate) access_expires_at: u64,
    pub(crate) refresh_token: Secret,
}

#[derive(Serialize)]
struct OAuthOut<'a> {
    v: u32,
    kind: CredentialKind,
    access_token: &'a str,
    access_expires_at: u64,
    refresh_token: &'a str,
}

#[derive(Serialize)]
struct ApiKeyOut<'a> {
    v: u32,
    kind: CredentialKind,
    key: &'a str,
}

/// Every field either form may hold; [`Credential::decode`] checks which are there.
#[derive(Deserialize)]
struct In {
    v: Option<u32>,
    kind: Option<CredentialKind>,
    access_token: Option<WireSecret>,
    access_expires_at: Option<u64>,
    refresh_token: Option<WireSecret>,
    key: Option<WireSecret>,
}

/// A string read straight into a [`Secret`], so a refused item leaves no plain copy behind.
pub(crate) struct WireSecret(pub(crate) Secret);

impl<'de> Deserialize<'de> for WireSecret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WireSecret, D::Error> {
        String::deserialize(deserializer).map(|text| WireSecret(Secret::new(text)))
    }
}

impl Credential {
    /// The credential's kind.
    pub(crate) fn kind(&self) -> CredentialKind {
        match self {
            Credential::OAuth(_) => CredentialKind::OAuth,
            Credential::ApiKey(_) => CredentialKind::ApiKey,
        }
    }

    /// The keychain item's bytes, in a buffer zeroed on drop. Refuses over
    /// [`MAX_SECRET_BYTES`] with `StoreError::TooLarge`.
    pub(crate) fn encode(&self) -> Result<Zeroizing<Vec<u8>>, StoreError> {
        let secrets = match self {
            Credential::OAuth(tokens) => tokens.access_token.len() + tokens.refresh_token.len(),
            Credential::ApiKey(key) => key.len(),
        };
        // JSON escapes at most six bytes per input byte, so the buffer never grows (a regrowth
        // would free a copy unzeroed).
        let mut out = Zeroizing::new(Vec::with_capacity(
            secrets.saturating_mul(6).saturating_add(160),
        ));
        let written = match self {
            Credential::OAuth(tokens) => serde_json::to_writer(
                &mut *out,
                &OAuthOut {
                    v: VERSION,
                    kind: CredentialKind::OAuth,
                    access_token: tokens.access_token.expose(),
                    access_expires_at: tokens.access_expires_at,
                    refresh_token: tokens.refresh_token.expose(),
                },
            ),
            Credential::ApiKey(key) => serde_json::to_writer(
                &mut *out,
                &ApiKeyOut {
                    v: VERSION,
                    kind: CredentialKind::ApiKey,
                    key: key.expose(),
                },
            ),
        };
        written.map_err(|err| {
            StoreError::Platform(format!("encoding failed ({:?})", err.classify()))
        })?;
        check_size(&out)?;
        Ok(out)
    }

    /// The credential in a keychain item, or `None` when the item does not parse, is over
    /// [`MAX_SECRET_BYTES`], has another version, or lacks a field: the caller deletes it.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Credential> {
        if bytes.len() > MAX_SECRET_BYTES {
            return None;
        }
        let item: In = serde_json::from_slice(bytes).ok()?;
        if item.v != Some(VERSION) {
            return None;
        }
        match item.kind? {
            CredentialKind::OAuth => {
                let access_token = item.access_token?.0;
                let refresh_token = item.refresh_token?.0;
                if access_token.is_empty() || refresh_token.is_empty() {
                    return None;
                }
                Some(Credential::OAuth(OAuthTokens {
                    access_token,
                    access_expires_at: item.access_expires_at?,
                    refresh_token,
                }))
            }
            CredentialKind::ApiKey => {
                let key = item.key?.0;
                (!key.is_empty()).then_some(Credential::ApiKey(key))
            }
        }
    }
}

/// `^cdk_live_[0-9A-Za-z]{32}$`.
pub(crate) fn is_api_key(key: &str) -> bool {
    key.strip_prefix("cdk_live_")
        .is_some_and(|tail| tail.len() == 32 && tail.bytes().all(|b| b.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests;
