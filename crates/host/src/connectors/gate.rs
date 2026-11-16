//! What both loopback listeners of a launch check before a client is let in: the case-law relay
//! (`relay.rs`) and the version-history listener (`versions_relay.rs`). One copy of each part:
//!
//! - [`loopback_only`]: a listener bound anywhere but loopback is refused before it accepts.
//! - [`TokenGate`]: the listener's token, kept only as its SHA-256, and its [`Phase`], under one
//!   lock, so a handshake sees both or neither change. A presented token is compared in constant
//!   time; once the token is revoked, every token is refused the same way.
//! - The strict hello and its one answer to every bad hello: `relay/handshake.rs` (`greet`).
//! - The accept loop: `relay/accept.rs`.

use std::io;
use std::net::SocketAddr;
use std::sync::{Mutex, MutexGuard};

use clauseo_connectors::Secret;
use sha2::{Digest as _, Sha256};

use super::relay::lock;
use super::texts;

/// Where a listener is in its life. Only forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Phase {
    Open,
    /// The token is revoked: handshakes get `closed`, and nothing more is answered.
    Ending,
    /// The listener is closing.
    Stopped,
}

/// The token's digest and the phase, under one lock.
pub(crate) struct TokenGate(Mutex<GateState>);

/// What [`TokenGate`] guards.
pub(crate) struct GateState {
    phase: Phase,
    /// `None` once the token is revoked.
    digest: Option<[u8; 32]>,
}

impl TokenGate {
    /// An open gate for `token`; only its digest is kept.
    pub(crate) fn new(token: &Secret) -> TokenGate {
        TokenGate(Mutex::new(GateState {
            phase: Phase::Open,
            digest: Some(digest(token.expose().as_bytes())),
        }))
    }

    pub(crate) fn phase(&self) -> Phase {
        lock(&self.0).phase
    }

    /// Whether `token` is this listener's live token, in constant time. `Err(())` once the
    /// token is revoked: the session is ending.
    pub(crate) fn token_matches(&self, token: &str) -> Result<bool, ()> {
        let presented = digest(token.as_bytes());
        let gate = lock(&self.0);
        match (gate.phase, gate.digest.as_ref()) {
            (Phase::Open, Some(ours)) => Ok(same_digest(ours, &presented)),
            _ => Err(()),
        }
    }

    /// Moves to `to` (never back) and forgets the token.
    pub(crate) fn close(&self, to: Phase) {
        let mut gate = lock(&self.0);
        gate.phase = gate.phase.max(to);
        gate.digest = None;
    }

    /// The gate's lock while it is open, so a connection is let in before it can close;
    /// `closed` when it is not open.
    pub(crate) fn while_open(&self) -> Result<MutexGuard<'_, GateState>, &'static str> {
        let gate = lock(&self.0);
        if gate.phase == Phase::Open {
            Ok(gate)
        } else {
            Err(texts::CLOSED)
        }
    }
}

/// `Ok` when `addr` is on loopback; otherwise `InvalidInput` with `refusal`, so nothing binds
/// a listener elsewhere.
pub(crate) fn loopback_only(addr: SocketAddr, refusal: &'static str) -> io::Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, refusal))
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Compares two digests in constant time.
fn same_digest(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let diff = left
        .iter()
        .zip(right)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b));
    std::hint::black_box(diff) == 0
}
