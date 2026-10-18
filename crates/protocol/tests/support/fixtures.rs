//! Shared test fixtures: deterministic ids built from small numbers, never from the clock.
//!
//! Each test file includes it as below. `pub` keeps a file that uses only some helpers free of
//! dead-code warnings, and `cfg(test)` lets the helpers `unwrap` under the workspace lints.
//!
//! ```text
//! #[cfg(test)]
//! #[path = "support/fixtures.rs"]
//! pub mod fixtures;
//! ```

use clauseo_protocol::{EventId, MatterId, MessageId, ThreadId, TurnId};
use uuid::Uuid;

/// A valid v7 UUID whose low bits are `n`: version nibble 7, RFC 4122 variant bits, and a fixed
/// timestamp, so the same `n` always gives the same id.
pub fn v7(n: u64) -> Uuid {
    // 48-bit timestamp | version 7 | 12 bits zero | variant 0b10 | 62 bits of n.
    let timestamp: u128 = 0x0190_0000_0000;
    let bits = (timestamp << 80) | (0x7 << 76) | (0b10 << 62) | u128::from(n & ((1 << 62) - 1));
    Uuid::from_u128(bits)
}

/// The hyphenated text of `v7(n)`.
pub fn v7_text(n: u64) -> String {
    v7(n).hyphenated().to_string()
}

/// A `ThreadId` from `v7(n)`.
pub fn thread_id(n: u64) -> ThreadId {
    ThreadId::new(v7(n)).unwrap()
}

/// A `MatterId` from `v7(n)`.
pub fn matter_id(n: u64) -> MatterId {
    MatterId::new(v7(n)).unwrap()
}

/// A `TurnId` from `v7(n)`.
pub fn turn_id(n: u64) -> TurnId {
    TurnId::new(v7(n)).unwrap()
}

/// A `MessageId` from `v7(n)`.
pub fn message_id(n: u64) -> MessageId {
    MessageId::new(v7(n)).unwrap()
}

/// An `EventId` from `v7(n)`.
pub fn event_id(n: u64) -> EventId {
    EventId::new(v7(n)).unwrap()
}
