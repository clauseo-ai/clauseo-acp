//! `clauseo-codex_appserver`: the Codex app-server messages the usage reader needs, and a
//! JSON-RPC client for them over any pair of async pipes.
//!
//! # Owns
//!
//! - Hand-written serde types for the app-server messages the usage reader sends and receives,
//!   named as in the app-server JSON schema at openai/codex tag `rust-v0.162.1`, the version
//!   codex-acp bundles (`types.rs`). The schema files the drift test reads are vendored in
//!   `schema/rust-v0.162.1/` (the test reads the tag from `SCHEMA_TAG`); `schema/README.md`
//!   gives their source, hashes and refresh steps.
//! - The line format: one JSON message per line, each at most 8 MiB (`line.rs`). Written
//!   messages carry no `"jsonrpc"` member, as the schema's `JSONRPCRequest` and
//!   `JSONRPCNotification` define them and as upstream's `serialize_get_account_rate_limits`
//!   test (`app-server-protocol/src/protocol/common.rs`) shows the wire form. Read messages may
//!   carry `"jsonrpc": "2.0"` or not.
//! - The schema drift test (`tests/schema_drift.rs`).
//!
//! # Interface
//!
//! [`connect`] takes the app-server's stdin and stdout and gives a [`Client`] (`initialize`,
//! `account`, `rate_limits`), the [`Notifications`] stream and the [`Driver`] future the caller
//! runs. Every failure is an [`Error`] with [`Error::code`]. The driver ends, and the stream
//! with it, once the last [`Client`] is dropped: hold a `Client` for as long as you want
//! notifications.
//!
//! # Invariants
//!
//! - No spawn, no sleep, no clock, no executor: all work happens inside [`Driver`]. The caller
//!   owns the process and every timeout.
//! - The caller starts `codex app-server` with Clauseo's own `CODEX_HOME` (S13's reader takes it
//!   as an input; #160). This crate never reads `~/.codex` and never chooses a home.
//! - Responses are matched to calls by id, in any order. No call blocks another. A call whose
//!   future is dropped before the driver writes it is never written, and its answer, if one
//!   comes, is discarded.
//! - [`Client::initialize`] sends empty `capabilities`, never `experimentalApi`: at the pinned
//!   version neither read nor `account/rateLimits/updated` is experimental.
//! - A request from the server is answered with JSON-RPC error -32601. A notification other
//!   than `account/rateLimits/updated` reaches [`Notifications`] as [`Notification::Other`]
//!   with its params dropped, and logs a `tracing` debug line. An `account/rateLimits/updated`
//!   whose params do not decode, and a response to no pending call, are skipped with a debug
//!   line.
//! - [`Notifications`] is unbounded and the driver never waits on it; dropping it discards
//!   later notifications.
//! - A line that is not a JSON-RPC message, or one over 8 MiB, ends the connection: every call
//!   made before then, written or still queued, fails with [`Error::Protocol`] and the driver
//!   ends with it. When stdout ends they fail with [`Error::Closed`] and the driver ends with
//!   `Err(Error::Closed)`. A call made after the driver ended fails with [`Error::Closed`]. A
//!   response that does not decode as its call's type fails that call alone, with
//!   [`Error::Protocol`].
//! - Unknown fields are ignored, and an unknown value of any enum decodes to its `Unknown`
//!   variant, which keeps the string.
//!
//! # Depends on
//!
//! `protocol` (for [`ErrorCode`](clauseo_protocol::ErrorCode)), `serde`, `serde_json`,
//! `futures`, `tracing`, `thiserror`. Never `tauri` or `tokio`, and no `codex-*` crate: the
//! caller hands over the pipes.
//!
//! # Start reading
//!
//! `driver.rs` (the two loops and the message kinds), then `client.rs`, `types.rs` and its
//! modules. Tests: `tests/client.rs` plays the app-server over in-memory pipes with the
//! fixtures in `tests/fixtures/`; `tests/schema_drift.rs` checks the types and the fixtures
//! against the vendored schema.

mod client;
mod driver;
mod error;
mod line;
mod types;

pub use client::{Client, Notification, Notifications};
pub use driver::Driver;
pub use error::Error;
pub use types::{
    Account, AccountRateLimitsUpdatedNotification, AmazonBedrockAccount, ChatgptAccount,
    ClientInfo, CreditsSnapshot, GetAccountParams, GetAccountRateLimitsParams,
    GetAccountRateLimitsResponse, GetAccountResponse, InitializeCapabilities, InitializeParams,
    InitializeResponse, PlanType, RateLimitReachedType, RateLimitResetCredit,
    RateLimitResetCreditStatus, RateLimitResetCreditsSummary, RateLimitResetType,
    RateLimitSnapshot, RateLimitWindow, SpendControlLimitSnapshot,
};

/// Starts a connection over the app-server's stdin (`writer`) and stdout (`reader`). Nothing is
/// sent until the caller runs the [`Driver`]; calls made before then wait for it.
pub fn connect<W, R>(writer: W, reader: R) -> (Client, Notifications, Driver)
where
    W: futures::io::AsyncWrite + Send + Unpin + 'static,
    R: futures::io::AsyncRead + Send + Unpin + 'static,
{
    let (commands, command_receiver) = futures::channel::mpsc::unbounded();
    let (notifications, notification_receiver) = futures::channel::mpsc::unbounded();
    (
        Client::new(commands),
        Notifications(notification_receiver),
        Driver::new(writer, reader, command_receiver, notifications),
    )
}
