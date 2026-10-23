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
//! Everything public is re-exported here; modules are private. Start with [`Envelope`] and
//! [`DomainEvent`] (one payload struct per kind, `DomainEvent::KINDS`, `DomainEvent::kind`),
//! then [`StreamId`], [`Seq`], the uuid ids such as [`ThreadId`], [`ContentBlock`],
//! [`RawPayload`], [`TextOrBlob`], [`AccountLimits`], [`ErrorCode`] and [`HostError`]. Clients
//! send a [`CommandEnvelope`] around a [`HostCommand`] (the host takes only the
//! [`ValidatedCommand`] that [`CommandEnvelope::validate`] makes, and answers with a
//! [`CommandOutcome`]) and read with a [`HostQuery`] ([`QueryResult`]). Subscribers receive
//! [`HostEvent`]s: [`PatchBatch`]es of [`ViewPatch`]es over [`ViewEntry`]s (keyed by
//! [`ViewEntry::id`], sorted by [`ViewEntry::order`]), or envelopes; [`Subscribe`],
//! [`SubscribeEvents`] and [`Ack`] are the subscription calls' parameters. The serde
//! form is the wire form: snake case, external tagging (a unit variant is a plain string, a data
//! variant a one-key object, an event `{"<kind>": {...}}`), `None` fields left out, and
//! `StreamId` and `EntryId` as one plain string `<kind>:<uuid>`. Events accept fields they do
//! not know; commands refuse them. Unit-only enums with an `ALL` constant list every value, and
//! enums with data list their variants' Rust names in `VARIANT_NAMES`, for tests that keep docs
//! true. Under the `specta` feature (off by default; only the app turns it on) every wire type
//! derives `specta::Type`, `wire_types()` registers them all, and `WireFormat` is the format that
//! exports them as their JSON form; `known_kinds_typescript` is the line of known event kinds an
//! app appends to that export. The one list of wire types is the `wire_types!` call below.
//!
//! # Invariants
//!
//! - No I/O, no clock, no randomness, no id minting: the host mints ids and passes time in.
//! - Constructors and `Deserialize` check the same rules (the webview is untrusted): Clauseo ids
//!   are v7 UUIDs, `Seq` starts at 1, and every 64-bit number stays at most [`MAX_SAFE`]
//!   (2^53 - 1), so TypeScript reads it as a `number`: counts and sizes are [`Count`], which
//!   cannot hold a larger value and refuses one on decode; producers of agent-reported counts
//!   cap them with [`Count::clamped`]. Floats are [`Finite`], which never holds NaN or an
//!   infinity (JSON writes those as `null`, which does not decode back).
//! - Serialising an `Unknown` whose kind is one of its type's known kinds is an error, so the
//!   log never holds an entry that would decode as a different, malformed variant.
//! - An envelope's `schema_version` picks how its `event` is read, in either key order: an
//!   `event` key before it is held as a [`RawPayload`] until the version is read. A store
//!   decodes a row as [`EnvelopeRead`]: one newer than [`SCHEMA_VERSION`] is
//!   [`EnvelopeRead::Newer`], and the store returns `store.newer_version` for it; a decode error
//!   is `store.corrupt`. An older one is upcast on read, in `envelope.rs`'s `upcast`.
//! - Matter-level events ([`DomainEvent::is_matter_level`]) go in a matter's stream, every other
//!   known kind in a thread's; an unknown kind goes where its envelope's `stream` says.
//! - Events are append-only and decode in one pass. A newer value in an enum that decodes unknown
//!   values becomes its catch-all (`Unknown`, `Unsupported`) instead of failing, and an unknown
//!   payload held as [`RawPayload`] writes back as the same value: JSON floats within one ULP,
//!   `MessagePack` byte for byte only for what rmp-serde writes (canonical integers, 64-bit
//!   floats), and a one-way form for what has none (a JSON copy of `MessagePack` input writes
//!   NaN as `null` and a scalar or binary key as a string; decoding refuses a list or map key).
//!   It decodes up to [`MAX_RAW_DEPTH`] lists and maps, as does an unknown kind's payload under
//!   a catch-all, and encodes up to [`MAX_RAW_ENCODE_DEPTH`], except a catch-all's payload.
//!   Every reader of `MessagePack` sets [`MAX_MSGPACK_DEPTH`] on its decoder, which bounds the
//!   fields an event does not know.
//! - Error codes are append-only; a code's meaning never changes.
//! - `validate()` checks shape only (lengths, counts, formats); state checks are the host's.
//! - A view entry's inline cost ([`ViewEntry::inline_len`]) is UTF-8 bytes after JSON escaping,
//!   without the quotes, summed over the entry's `Inline` values, `Vec` items included; the host
//!   keeps it within [`INLINE_LIMIT_BYTES`]. Large variants are boxed (`ViewPatch` <= 64 bytes).
//! - [`SCHEMA_VERSION`] versions the stored log: [`Envelope`], [`DomainEvent`] and every type
//!   they hold. A new kind, a new optional field or a new variant of an open enum keeps it.
//!   Renaming, removing or retyping a logged field, variant or kind, or a new variant of one of
//!   the closed enums ([`TextOrBlob`], [`RawPayload`], [`StreamId`]), bumps it, with a step
//!   added to `upcast` in `envelope.rs`. View, query and command types are outside it (0002,
//!   Freeze rules).
//! - What `store` keeps outside the log is disposable or counts as missing. The reducer's
//!   snapshots (view entries) and the limits cache ([`AccountLimits`]) are keyed by the build
//!   that wrote them: a build mismatch or a value that does not decode means rebuild the snapshot
//!   from the log, or read the limits again. A receipt ([`CommandOutcome`]) that does not decode
//!   counts as missing, which allows one re-run per `command_id`. So a closed enum in them
//!   (`ThreadStatus`, `StaleReason`) may gain a variant, and an open enum's catch-all written
//!   back in its plain form is safe in a snapshot: `{"crashed":{"signal":9}}`, read by an older
//!   build as [`DetachReason`]'s `Unknown` and written as `"crashed"`, fails to decode in the
//!   newer build, which rebuilds the snapshot from the log.
//!
//! # Depends on
//!
//! `serde`, `serde_json`, `uuid`, `compact_str`, `thiserror`, and `specta` and `specta-serde`
//! behind the `specta` feature.
//! Never `tauri`, `tokio`, the file system, the network or processes (`ARCHITECTURE.md`).
//!
//! # Start reading
//!
//! `envelope.rs` (its decoder in `envelope/de.rs`) and `event.rs` (the kind table) for the log,
//! `event/de.rs` for the one-pass decoder, `ids.rs` (short-string ids in `ids/strings.rs`),
//! `number.rs` and `stream.rs` for the ids and numbers, `raw.rs` for unknown payloads, `text.rs`
//! for inline costs, `title.rs` for the title rules, `open_enum.rs` for how unknown variants
//! decode, `error.rs` and `error_code.rs` for errors and the codes, `limits.rs` for account
//! limits, `command.rs` and `command/validate.rs` for commands and their limits, `query.rs`,
//! `view.rs` (entries in `view/entry.rs`) and `patch.rs` for what subscribers receive, `wire.rs`
//! for the TypeScript export's rules.
//!
//! Every event payload is in `event/payload/`; the value types inside them live by topic.
//!
//! Tests: private helpers in `src/<mod>/tests.rs`; the rest under `tests/`. Goldens in
//! `tests/snapshots.rs`; events, envelopes and rules in `tests/events.rs`, with catch-all enums
//! in `tests/events/unknown_variants.rs` and `tests/events/open_enums.rs` (each open enum's
//! round trip is a test its macro writes beside it), encoder and decoder rules in
//! `tests/events/rules.rs`, JSON copies of `MessagePack` values in `tests/events/json_copies.rs`,
//! `Count` and `Finite` in `tests/events/numbers.rs` and the payload constructors that refuse
//! meaningless combinations in `tests/events/constructors.rs`; the frozen v1 log entries that must
//! keep decoding in `tests/corpus.rs`; command and query limits in `tests/commands.rs`, typed
//! fields and name rules in `tests/commands/names.rs`, file lists in `tests/commands/files.rs`
//! and elicitation answers in `tests/commands/elicitation.rs`; view entries and patches in
//! `tests/view.rs`; inline costs in `tests/text.rs`; the TypeScript export in
//! `tests/specta_export.rs`; the docs the tests keep true in `tests/ui_states.rs`,
//! `tests/error_codes.rs` and `tests/enum_table.rs` (0002's enum table). Shared fixtures and
//! examples are in `tests/support/`. Adding a kind, a variant or an open enum touches several of
//! these: 0002's checklist, at the end of Freeze rules, names each place and the test that
//! fails when it is missed.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod approval;
mod command;
mod content;
mod envelope;
mod error;
mod error_code;
mod event;
mod ids;
mod limits;
mod matter;
mod number;
mod open_enum;
mod patch;
mod query;
mod raw;
mod serde_util;
mod session;
mod stream;
mod text;
mod title;
mod tool;
mod turn;
mod view;

/// Writes the one list of wire types: a `pub use <module>::{..};` per module and, under the
/// `specta` feature, [`wire_types`], which registers every one of them.
macro_rules! wire_types {
    ($($m:ident::{$($t:ident),+ $(,)?}),+ $(,)?) => {
        $(pub use $m::{$($t),+};)+

        /// Every wire type, each registered with what it reaches: what an app exports, with
        /// [`WireFormat`]. Each type exports under its Rust name.
        #[cfg(feature = "specta")]
        #[must_use]
        pub fn wire_types() -> specta::Types {
            specta::Types::default()$($(.register::<$t>())+)+
        }
    };
}

// Every type that crosses the boundary: each derives `specta::Type` under the `specta` feature.
wire_types! {
    approval::{
        Deliverable, ElicitationOutcome, ElicitationRequest, ElicitationResponse, ExpiryReason,
        PermissionChoice, PermissionKind, PermissionOption, Principal,
    },
    command::{
        CommandEnvelope, CommandOutcome, ConfigValue, DeliveryRequest, HostCommand, MatterSource,
        RestoreRequest,
    },
    content::{ContentBlock, PromptBlock, RawFrom, ToolCallContent},
    envelope::{Envelope},
    error_code::{ErrorCode, HostError},
    event::{
        AgentSessionAttached, AgentSessionDetached, CheckpointRecorded, CommandsAvailable,
        CompactionChanged, ConfigOptionsReplaced, ContentAppended, DomainEvent, ElicitationExpired,
        ElicitationRequested, ElicitationResolved, FilesChanged, MatterCreated, ModeChanged,
        Notice, OutsideEdits, PermissionExpired, PermissionRequested, PermissionResolved,
        PlanReplaced, QueuedPromptEdited, SteerResolved, StepUndone, TextAppended, ThreadArchived,
        ThreadCreated, ThreadRenamed, ToolCallStarted, ToolCallUpdated, TurnEnded, TurnFailed,
        TurnQueued, TurnStarted, TurnStoppedAtLimit, UsageUpdated, UserMessage, VersionRestored,
    },
    ids::{
        AgentChoiceId, AgentId, AgentSessionId, BlobHash, CheckpointId, CommandId, ContentId,
        ElicitationId, EventId, FileRefId, FolderRefId, MatterId, MatterPath, MessageId,
        PermissionRequestId, ThreadId, ToolCallId, TurnId, ViewEpoch,
    },
    limits::{
        AccountLimits, AccountRef, AgentKind, AgentLimitsView, AtLimitBehaviour, Billing,
        BillingKind, BillingProvider, Freshness, LimitMeter, LimitSource, LimitStopReason,
        LimitWindow, LimitWindowView, LimitsView, MeterTone, PlanLabel, StaleReason,
        UnavailableReason, WindowFreshness, WindowKind,
    },
    matter::{CheckpointKind, FileChoice, FileDecision, ImportSummary, MatterKind, RestoreScope},
    number::{Count, Cursor, Finite, Seq, Timestamp},
    patch::{
        Ack, AppendTarget, HostEvent, PatchBatch, StreamHeader, Subscribe, SubscribeEvents,
        ViewPatch, ViewPatchBody, ViewPosition, ViewWindow,
    },
    query::{
        BlobRange, HostQuery, MatterSummary, PageLimit, QueryResult, ReadLen, ThreadSummary,
    },
    raw::{RawPayload},
    session::{
        AgentCapabilities, AvailableCommand, CompactionStatus, ConfigCategory, ConfigChoice,
        ConfigOption, ConfigOptionKind, Cost, DetachReason, NoticeLevel, PlanEntry, PlanPriority,
        PlanStatus, PromptCapabilities, RenameSource, SessionCapabilities, SteeringCapability,
        TextKind, TurnUsage,
    },
    stream::{StreamId},
    text::{BlobRef, TextOrBlob},
    title::{Title},
    tool::{ToolCallPatch, ToolCallStatus, ToolKind, ToolLocation},
    turn::{
        ChangeKind, Delivery, FileChange, SkippedFile, SkippedFileReason, SteerOutcome, StopReason,
    },
    view::{
        AgentContentEntry, AgentTextEntry, ContextUsage, ElicitationEntry, ElicitationState,
        EntryId, HeaderEntry, LimitStopEntry, MarkerEntry, MarkerKind, MatterHeaderEntry,
        NoticeEntry, PermissionEntry, PermissionState, StepEntry, ThreadStatus, ToolCallEntry,
        UserMessageEntry, UserMessageStatus, VersionEntry, ViewEntry,
    },
}

// Re-exports that are not wire types: constants, functions, and types that never cross the
// boundary.
pub use command::{
    MAX_ELICITATION_VALUES_BYTES,
    MAX_FILE_LIST,
    MAX_MATTER_NAME_BYTES,
    MAX_PATH_LIST_BYTES,
    MAX_PROMPT_BLOCKS,
    MAX_PROMPT_FILES,
    MAX_PROMPT_TEXT_BYTES,
    MAX_QUEUED_TURNS,
    // A checked command inside Rust; never serialised (the client sends `CommandEnvelope`).
    ValidatedCommand,
};
// compact_str's type, re-exported for callers; wire fields holding it export as `string`.
pub use compact_str::CompactString;
// What a store decodes a stored envelope as; it is never written, so it has no wire form.
pub use envelope::EnvelopeRead;
// The crate's error; it never crosses the boundary (`HostError` does).
pub use error::Error;
pub use ids::{MAX_ID_TEXT_BYTES, is_windows_reserved_name};
pub use number::MAX_SAFE;
// The kind a catch-all holds; the catch-all variants holding it export as `string`.
pub use open_enum::UnknownKind;
pub use query::{MAX_BLOB_READ_BYTES, MAX_PAGE_ENTRIES};
pub use raw::{MAX_MSGPACK_DEPTH, MAX_RAW_DEPTH, MAX_RAW_ENCODE_DEPTH};
pub use text::{INLINE_LIMIT_BYTES, PREVIEW_BYTES, escaped_len, preview_of, utf16_len};
pub use title::{MAX_TITLE_CHARS, check_display_title};

/// The envelope's `schema_version` this crate writes: 1 at the freeze.
pub const SCHEMA_VERSION: u32 = 1;

/// A constant for xtask's `check_protocol` and `build_protocol` buildtime probes: they edit it to
/// force a rebuild of this crate. It means nothing at run time.
#[doc(hidden)]
pub const BUILDTIME_MARKER: u64 = 0;
