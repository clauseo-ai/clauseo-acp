//! The crate's errors. None of them holds a secret, a URL query or a response body: the strings
//! say what failed, never what was sent.

use crate::recharge::PaymentRef;

/// A failure of the keychain or of the state file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// This OS has no keychain store in this build (Linux, which is not a product target).
    #[error("no keychain store on this platform")]
    Unsupported,
    /// The value is over [`MAX_SECRET_BYTES`](crate::MAX_SECRET_BYTES); nothing was written.
    #[error("the keychain value is {len} bytes, over the limit")]
    TooLarge {
        /// The value's length in bytes.
        len: usize,
    },
    /// The keychain refused access (locked, or the lawyer denied the prompt).
    #[error("the keychain refused access: {0}")]
    NoAccess(String),
    /// Any other keychain failure, by kind.
    #[error("the keychain failed: {0}")]
    Platform(String),
    /// The state file could not be written.
    #[error("the connectors' state file could not be written: {0}")]
    StateFile(String),
}

/// Why no bearer token can be given.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The connector was never added on this install.
    #[error("the connector is not added")]
    NotAdded,
    /// The sign-in ended: signed out, or the refresh token was refused.
    #[error("signed out of the connector")]
    SignedOut,
    /// The API key was refused.
    #[error("the connector's API key was revoked")]
    KeyRevoked,
    /// The keychain failed.
    #[error(transparent)]
    Store(StoreError),
    /// The refresh could not reach the connector; the credential is kept.
    #[error("network: {0}")]
    Network(String),
}

/// Why a sign-in failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignInError {
    /// The connector's metadata failed a check, or could not be read.
    #[error("discovery failed: {0}")]
    Discovery(String),
    /// The lawyer cancelled, in the browser or in the app.
    #[error("sign-in cancelled")]
    Cancelled,
    /// No callback came before the timeout.
    #[error("sign-in timed out")]
    TimedOut,
    /// The callback's `state` was wrong.
    #[error("the sign-in callback's state did not match")]
    StateMismatch,
    /// The callback's `iss` was missing or not the issuer.
    #[error("the sign-in callback's issuer did not match")]
    IssuerMismatch,
    /// The authorization server answered with an error, or the token exchange failed.
    #[error("token exchange failed: {0}")]
    Exchange(String),
    /// The credential could not be stored.
    #[error(transparent)]
    Store(StoreError),
    /// Another sign-in of this connector is running.
    #[error("another sign-in is running")]
    Busy,
}

/// A failed call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No usable credential.
    #[error(transparent)]
    Auth(AuthError),
    /// The connector answered with this status.
    #[error("the connector answered HTTP {status}")]
    Http {
        /// The HTTP status.
        status: u16,
    },
    /// The connector could not be reached.
    #[error("network: {0}")]
    Network(String),
    /// The answer did not parse.
    #[error("the connector's answer did not parse: {0}")]
    Parse(String),
    /// Out of credit: the JSON API's 402, with the payment it opened.
    #[error("out of credit")]
    InsufficientCredits(PaymentRef),
    /// The connector sent a pay link that is not ours.
    #[error("the pay link is not the connector's own")]
    NotOurPayLink,
    /// A pasted API key is not `cdk_live_` and 32 base62 characters.
    #[error("not a connector API key")]
    BadKeyFormat,
    /// The recharge was not paid before the deadline.
    #[error("the recharge was not paid in time")]
    RechargeTimedOut,
    /// The cancel flag was set.
    #[error("cancelled")]
    Cancelled,
    /// The keychain or the state file failed.
    #[error(transparent)]
    Store(StoreError),
    /// The configured origin is not allowed (a fake origin off `http://127.0.0.1:<port>`).
    #[error("the connector origin is not allowed")]
    BadOrigin,
}

impl From<StoreError> for AuthError {
    fn from(err: StoreError) -> AuthError {
        AuthError::Store(err)
    }
}

impl From<StoreError> for SignInError {
    fn from(err: StoreError) -> SignInError {
        SignInError::Store(err)
    }
}

impl From<StoreError> for Error {
    fn from(err: StoreError) -> Error {
        Error::Store(err)
    }
}

impl From<AuthError> for Error {
    fn from(err: AuthError) -> Error {
        Error::Auth(err)
    }
}

/// A serde error as a `Parse` message: its category and position only, never the text it read
/// (serde's own message can quote a value).
pub(crate) fn parse_error(what: &str, err: &serde_json::Error) -> Error {
    let kind = match err.classify() {
        serde_json::error::Category::Io => "read error",
        serde_json::error::Category::Syntax => "not JSON",
        serde_json::error::Category::Data => "unexpected shape",
        serde_json::error::Category::Eof => "truncated",
    };
    Error::Parse(format!(
        "{what}: {kind} at line {} column {}",
        err.line(),
        err.column()
    ))
}
