//! The paid connectors in the host (R24). This part is the case-law relay: one loopback listener
//! per agent launch, which checks `clauseo-mcp connector`'s handshake, holds the input limits
//! and the spend caps, and forwards each JSON-RPC line to the connector through
//! `clauseo-connectors` with the lawyer's credential, which never leaves the host.
//!
//! - [`Relays`], one per app, holds what every listener shares: the connector's handle, the day
//!   count, the clock, the local day and the events sink.
//! - [`Relays::open`] starts a [`RelaySession`] on a bound listener and mints its token, a
//!   [`Secret`] the caller writes to the token file. The relay keeps only its SHA-256 and
//!   compares in constant time, so a token of another listener is `bad_token`.
//! - [`RelaySession::end`] revokes the token first (handshakes then get `closed`) and closes the
//!   connections; dropping it stops the listener, and nothing is accepted after that.
//! - [`TurnHandle`] is written by the host thread when a turn starts and ends; a relay thread
//!   reads it when it counts a call.
//! - [`RelayEvents`] hears what the host shows: a credential that ended, out of credit, and a
//!   paid result (a balance read is due). [`ConnectorEvents`] is the host's own, bound to its
//!   actor at start (`wiring.rs`).
//! - `hint.rs` decides when a turn's web searches look like case law (the hint, PR6); the
//!   relay's `CaseLawFound` feeds the thread's sources (`sources/case_law.rs`).
//! - `state.rs` holds what the app sees of the connector (PR5): the credential, the balance, out
//!   of credit and the pay link; `actor/connectors.rs` runs the commands that change it.
//!
//! Logs name the session, the method and the reason, never a line's content, an argument, a
//! URL query or a token; the hello line is never logged or echoed.
//!
//! [`Secret`]: clauseo_connectors::Secret

pub(crate) mod hint;
mod limits;
mod relay;
pub(crate) mod state;
mod texts;
mod wiring;

use std::time::Duration;

use clauseo_connectors::PaymentRef;

use crate::config::SessionKey;

pub use limits::{
    FixedOffset, HANDSHAKE_TIMEOUT, LocalDay, MAX_CONNECTIONS_PER_TOKEN, MAX_HELLO_BYTES,
    MAX_IN_FLIGHT, MAX_IN_FLIGHT_BYTES_PER_SESSION, MAX_LINE_BYTES, MAX_PAID_CALLS_PER_DAY,
    MAX_PAID_CALLS_PER_TURN, MAX_PENDING_HANDSHAKES, MAX_RECHARGE_CALLS_PER_TURN, MAX_REPLY_BYTES,
};
pub use relay::{RelayConfig, RelaySession, Relays, TurnHandle};
pub use wiring::{ConnectorEvents, ConnectorOpener, ConnectorsWiring};

/// What the relay tells the host, from the relay's own threads.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelayEvent {
    /// A `tools/call` found no usable credential, or the connector refused it after the
    /// refresh and retry.
    CredentialEnded {
        /// The session whose call it was.
        session: SessionKey,
        /// How it ended.
        ended: CredentialEnd,
    },
    /// A `tools/call` came back out of credit, with the payment the connector opened (its pay
    /// link only when it is the connector's own).
    OutOfCredit {
        /// The session whose call it was.
        session: SessionKey,
        /// The recharge and its checked pay link.
        payment: PaymentRef,
    },
    /// A paid tool answered without an error: the balance changed.
    PaidResult {
        /// The session whose call it was.
        session: SessionKey,
    },
    /// A `search_law` or `read_document` call answered without an error: what it found, for
    /// the thread's sources (`sources/case_law.rs` joins it to the agent's tool call).
    CaseLawFound {
        /// The session whose call it was.
        session: SessionKey,
        /// What the call found.
        found: CaseLawFound,
    },
}

/// The case-law tools whose results become sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseLawTool {
    /// `search_law`: a `WebSearchRecorded { source: CaseLaw }`.
    SearchLaw,
    /// `read_document`: one `PageVisited { via: CaseLaw }` per document.
    ReadDocument,
}

impl CaseLawTool {
    /// The tool by the connector's name for it; `None` for any other tool.
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "search_law" => Some(Self::SearchLaw),
            "read_document" => Some(Self::ReadDocument),
            _ => None,
        }
    }

    /// The connector's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Self::SearchLaw => "search_law",
            Self::ReadDocument => "read_document",
        }
    }
}

/// What one case-law call found, as the relay read it from the result's `structuredContent`
/// (`results[]` for a search, `documents[]` for a read: the fake connector's paths, not yet
/// checked against the live connector). Titles and links are as the connector gave them, cut to
/// the record's caps; the sources join cleans and checks them.
#[derive(Clone, PartialEq, Eq)]
pub struct CaseLawFound {
    /// The tool that ran.
    pub tool: CaseLawTool,
    /// SHA-256 of the tool's name and the canonical JSON of its arguments (sorted keys): the join
    /// key, so the host keeps no argument text.
    pub key: [u8; 32],
    /// The titles and links, in the connector's order; at most `MAX_SEARCH_RESULTS`.
    pub links: Vec<FoundLink>,
}

impl std::fmt::Debug for CaseLawFound {
    // Titles and links stay out of logs: they can echo the lawyer's search words.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaseLawFound")
            .field("tool", &self.tool)
            .field("links", &self.links.len())
            .finish_non_exhaustive()
    }
}

/// One title and link a case-law call returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundLink {
    /// The title as the connector gave it, cut to `MAX_WEB_TITLE_CHARS`.
    pub title: String,
    /// The link as the connector gave it.
    pub url: String,
}

/// Why a `tools/call` had no usable credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialEnd {
    /// The connector is not added on this install.
    NotAdded,
    /// Signed out, or the refresh token was refused.
    SignedOut,
    /// The firm's API key was refused.
    KeyRevoked,
}

/// The host's ear on the relay. Called on relay threads; keep it quick.
pub trait RelayEvents: Send + Sync {
    /// One event.
    fn event(&self, event: RelayEvent);

    /// The relay's own time for one forwarded message: from its line read to the POST's start,
    /// plus from the reply to its last line written. The bench reads it; the default drops it.
    fn overhead(&self, took: Duration) {
        let _ = took;
    }
}
