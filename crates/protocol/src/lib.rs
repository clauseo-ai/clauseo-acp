//! `clauseo-protocol`: every type that crosses the host boundary.
//!
//! # Owns
//!
//! The event envelope and the domain events the log stores; the commands a client sends and
//! their shape validation; the queries; the view entries and view patches a subscriber receives;
//! ids and the other small validated values; error codes and `HostError`; account limit types;
//! the inline cost rules for large payloads (`TextOrBlob`, `escaped_len`, `utf16_len`). Decision
//! records 0002 (event schema) and 0003 (error codes) freeze the wire form.
//!
//! # Interface
//!
//! Everything public is re-exported here; modules are private. Start with [`StreamId`],
//! [`Seq`], the uuid ids such as [`ThreadId`], [`RawPayload`], [`TextOrBlob`], [`ErrorCode`] and
//! [`HostError`]. The serde form is the wire form: snake case, external tagging (a unit variant
//! is a plain string, a data variant a one-key object), `None` fields left out, and `StreamId` and
//! `EntryId` as one plain string `<kind>:<uuid>`. Under the `specta` feature (off by default; only
//! the app turns it on) every wire type derives `specta::Type`.
//!
//! # Invariants
//!
//! - No I/O, no clock, no randomness, no id minting: the host mints ids and passes time in.
//! - Constructors and `Deserialize` check the same rules (the webview is untrusted): Clauseo ids
//!   are v7 UUIDs, `Seq` starts at 1, and every 64-bit number stays at most [`MAX_SAFE`]
//!   (2^53 - 1), so TypeScript reads it as a `number`.
//! - Events are append-only and decode in one pass. A newer value in an enum that decodes unknown
//!   values becomes its catch-all (`Unknown`, `Unsupported`) instead of failing, and an unknown
//!   payload held as [`RawPayload`] writes back as it was read, in JSON and in `MessagePack`.
//! - Error codes are append-only; a code's meaning never changes.
//! - [`SCHEMA_VERSION`] changes only when a field is renamed, removed or retyped, with an upcaster
//!   here.
//!
//! # Depends on
//!
//! `serde`, `serde_json`, `uuid`, `compact_str`, `thiserror`, and `specta` behind its feature.
//! Never `tauri`, `tokio`, the file system, the network or processes (`ARCHITECTURE.md`).
//!
//! # Start reading
//!
//! `ids.rs` and `stream.rs` for the ids, `raw.rs` for unknown payloads, `text.rs` for inline
//! costs, `open_enum.rs` for how unknown variants decode, `error_code.rs` for the codes.

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod error;
mod error_code;
mod ids;
mod open_enum;
mod raw;
mod stream;
mod text;

pub use compact_str::CompactString;
pub use error::Error;
pub use error_code::{ErrorCode, HostError};
pub use ids::{
    AgentId, AgentSessionId, BlobHash, CheckpointId, CommandId, ContentId, Cursor, ElicitationId,
    EventId, FileRefId, FolderRefId, MAX_SAFE, MatterId, MatterPath, MessageId,
    PermissionRequestId, Seq, ThreadId, Timestamp, ToolCallId, TurnId, ViewEpoch,
};
pub use raw::RawPayload;
pub use stream::{EntryId, StreamId};
pub use text::{
    BlobRef, INLINE_LIMIT_BYTES, PREVIEW_BYTES, TextOrBlob, escaped_len, preview, utf16_len,
};

/// The envelope's `schema_version` this crate writes: 1 at the freeze.
pub const SCHEMA_VERSION: u32 = 1;

/// A constant for xtask's `check_protocol` and `build_protocol` buildtime probes: they edit it to
/// force a rebuild of this crate. It means nothing at run time.
pub const BUILDTIME_MARKER: u64 = 0;
