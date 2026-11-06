//! The paid connectors Clauseo runs at `data.clauseo.chat`: sign-in, the credential in the OS
//! keychain, the account, liveness and price. The first and only connector is case law
//! ([`CASE_LAW`]).
//!
//! Contract:
//! - Blocking I/O, no async runtime: no tokio, no hyper, no Tauri (a test reads `cargo metadata`).
//! - The credential lives in one keychain item per connector ([`KeyringStore`], service
//!   [`KEYCHAIN_SERVICE`]), never in settings, the state file, logs or an error. Every secret is a
//!   [`Secret`]: no `Display`, no `Serialize`, `Debug` prints `Secret(..)`, zeroed on drop.
//! - The state file (`<data_dir>/connectors/state.json`) holds no secret, only which connectors
//!   were added and how their credential ended; it is written atomically (`write.rs`).
//! - Only compiled-in connectors ([`KNOWN`]) and URLs exist; under a test or staging [`Origin`]
//!   every `https://data.clauseo.chat` URL maps onto that origin, keeping its path.
//! - A pay link leaves the crate only as a [`PayUrl`], which only our own `/pay/` URLs parse to.
//! - Time comes from [`Clock`]; waits are [`CancelFlag::wait_timeout`].
//!
//! The `test-support` feature adds the fake connector ([`fake`]), [`MemoryStore`],
//! [`ManualClock`] and `Origin::Fake`.

mod account;
mod calls;
mod cancel;
mod config;
mod connectors;
mod credential;
mod de;
mod error;
mod http;
mod known;
mod mcp;
mod net;
mod oauth;
mod pkce;
mod pricing;
mod recharge;
mod refresh;
mod secret;
mod state;
mod store;
mod write;

#[cfg(feature = "test-support")]
pub mod fake;

pub use account::{Account, DaySpend};
pub use calls::ACCOUNT_MIN_INTERVAL;
pub use cancel::CancelFlag;
#[cfg(feature = "test-support")]
pub use config::ManualClock;
pub use config::{Clock, Config, Origin, ProxyChoice, ProxyRefused, ProxyResolver, SystemClock};
#[cfg(feature = "test-support")]
pub use connectors::ListenerLoad;
pub use connectors::{ConnectorHandle, Connectors};
pub use credential::{CredentialKind, CredentialStatus, SignOutReport};
pub use error::{AuthError, Error, SignInError, StoreError};
pub use known::{CASE_LAW, CLIENT_ID, KEYCHAIN_SERVICE, KNOWN, KnownConnector, PRICING_URL, known};
pub use mcp::{
    McpBody, McpReply, McpRequestHeaders, ToolOutcome, classify_tool_result,
    classify_tool_result_for,
};
pub use net::bind_loopback_exclusive;
pub use oauth::PendingSignIn;
pub use pricing::{Liveness, Pack, Pricing, PricingStatus};
pub use recharge::{Amount, PayUrl, PaymentRef, Recharge, RechargeStatus};
pub use secret::{Bearer, Secret, new_relay_token};
#[cfg(feature = "test-support")]
pub use store::MemoryStore;
pub use store::{KeyringStore, MAX_SECRET_BYTES, SecretStore};
