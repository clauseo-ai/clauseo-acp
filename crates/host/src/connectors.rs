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
//!   paid result (a balance read is due).
//!
//! Logs name the session, the method and the reason, never a line's content, an argument, a
//! URL query or a token; the hello line is never logged or echoed.
//!
//! [`Secret`]: clauseo_connectors::Secret

mod limits;
mod relay;
mod texts;

use std::time::Duration;

use clauseo_connectors::PaymentRef;

use crate::config::SessionKey;

pub use limits::{
    FixedOffset, HANDSHAKE_TIMEOUT, LocalDay, MAX_CONNECTIONS_PER_TOKEN, MAX_HELLO_BYTES,
    MAX_IN_FLIGHT, MAX_IN_FLIGHT_BYTES_PER_SESSION, MAX_LINE_BYTES, MAX_PAID_CALLS_PER_DAY,
    MAX_PAID_CALLS_PER_TURN, MAX_PENDING_HANDSHAKES, MAX_RECHARGE_CALLS_PER_TURN, MAX_REPLY_BYTES,
};
pub use relay::{RelayConfig, RelaySession, Relays, TurnHandle};

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
