//! Shared test fixtures: deterministic ids built from small numbers, never from the clock, and
//! round-trip helpers for JSON and `MessagePack`. Example events are in `examples.rs`.
//!
//! Each test file includes it as below. `pub` keeps a file that uses only some helpers free of
//! dead-code warnings, and `cfg(test)` lets the helpers `unwrap` under the workspace lints.
//!
//! ```text
//! #[cfg(test)]
//! #[path = "support/fixtures.rs"]
//! pub mod fixtures;
//! ```

use std::fmt::Debug;

use clauseo_protocol::{
    AgentId, BlobHash, BlobRef, CheckpointId, CommandId, CompactString, DomainEvent, ElicitationId,
    Envelope, EventId, FileRefId, FolderRefId, MatterId, MatterPath, MessageId,
    PermissionRequestId, RawPayload, Seq, StreamId, ThreadId, Timestamp, ToolCallId, TurnId,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
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

/// A `ToolCallId` from `v7(n)`.
pub fn tool_call_id(n: u64) -> ToolCallId {
    ToolCallId::new(v7(n)).unwrap()
}

/// A `CheckpointId` from `v7(n)`.
pub fn checkpoint_id(n: u64) -> CheckpointId {
    CheckpointId::new(v7(n)).unwrap()
}

/// A `PermissionRequestId` from `v7(n)`.
pub fn permission_request_id(n: u64) -> PermissionRequestId {
    PermissionRequestId::new(v7(n)).unwrap()
}

/// An `ElicitationId` from `v7(n)`.
pub fn elicitation_id(n: u64) -> ElicitationId {
    ElicitationId::new(v7(n)).unwrap()
}

/// A `CommandId` from `v7(n)`.
pub fn command_id(n: u64) -> CommandId {
    CommandId::new(v7(n)).unwrap()
}

/// A `FileRefId` from `v7(n)`.
pub fn file_ref_id(n: u64) -> FileRefId {
    FileRefId::new(v7(n)).unwrap()
}

/// A `FolderRefId` from `v7(n)`.
pub fn folder_ref_id(n: u64) -> FolderRefId {
    FolderRefId::new(v7(n)).unwrap()
}

/// A `Seq` of `n`.
pub fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

/// A blob of `size_bytes` whose hash bytes are all `byte`, with a short preview.
pub fn blob(byte: u8, size_bytes: u64) -> BlobRef {
    BlobRef::new(
        BlobHash::from_bytes([byte; 32]),
        size_bytes,
        "preview".to_owned(),
    )
    .unwrap()
}

/// An `AgentId`.
pub fn agent(text: &str) -> AgentId {
    AgentId::new(text).unwrap()
}

/// What `serde_json` writes for `text`, in bytes, less the two quotes: the oracle for inline costs.
pub fn json_cost(text: &str) -> usize {
    serde_json::to_string(text).unwrap().len() - 2
}

/// A matter-relative path.
pub fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

/// A `CompactString`.
pub fn short(text: &str) -> CompactString {
    CompactString::from(text)
}

/// The `RawPayload` a JSON text holds, keys in their JSON order.
pub fn raw(json: &str) -> RawPayload {
    serde_json::from_str(json).unwrap()
}

/// The `MessagePack` form of the value a JSON text holds, keys in their JSON order.
pub fn msgpack_of(json: &str) -> Vec<u8> {
    rmp_serde::to_vec_named(&raw(json)).unwrap()
}

/// The value `to_vec_named` writes for `value`, read back as a `RawPayload` to edit.
pub fn packed_raw<T: Serialize>(value: &T) -> RawPayload {
    rmp_serde::from_slice(&rmp_serde::to_vec_named(value).unwrap()).unwrap()
}

/// The value of the string key `key` in the map `map`.
pub fn entry<'a>(map: &'a mut RawPayload, key: &str) -> &'a mut RawPayload {
    let RawPayload::Map(entries) = map else {
        panic!("not a map: {map:?}");
    };
    let found = entries
        .iter_mut()
        .find(|(k, _)| *k == RawPayload::Str(key.into()));
    &mut found.unwrap().1
}

/// Removes the string key `key` from the map `map`.
pub fn remove_entry(map: &mut RawPayload, key: &str) {
    let RawPayload::Map(entries) = map else {
        panic!("not a map: {map:?}");
    };
    let mut kept = entries.to_vec();
    kept.retain(|(k, _)| *k != RawPayload::Str(key.into()));
    *entries = kept.into();
}

/// Checks that `value` decodes back to itself from JSON and from `MessagePack`
/// (`to_vec_named`), and that each encoding of the decoded value gives the same bytes again.
pub fn round_trips<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let json = serde_json::to_string(value).unwrap();
    let from_json: T = serde_json::from_str(&json).unwrap();
    assert_eq!(&from_json, value, "JSON {json}");
    assert_eq!(serde_json::to_string(&from_json).unwrap(), json);
    let packed = rmp_serde::to_vec_named(value).unwrap();
    let from_packed: T = rmp_serde::from_slice(&packed).unwrap();
    assert_eq!(&from_packed, value, "MessagePack of {json}");
    assert_eq!(rmp_serde::to_vec_named(&from_packed).unwrap(), packed);
}

/// Decodes `json` as `T` from JSON and from its `MessagePack` form.
pub fn decode_both<T: DeserializeOwned>(json: &str) -> (Result<T, String>, Result<T, String>) {
    let from_json = serde_json::from_str::<T>(json).map_err(|e| e.to_string());
    let from_packed = rmp_serde::from_slice::<T>(&msgpack_of(json)).map_err(|e| e.to_string());
    (from_json, from_packed)
}

/// Checks that `json` decodes to `expected` from JSON and from its `MessagePack` form.
pub fn decodes_in_both<T: DeserializeOwned + PartialEq + Debug>(json: &str, expected: &T) {
    let (from_json, from_packed) = decode_both::<T>(json);
    assert_eq!(from_json.as_ref(), Ok(expected), "JSON {json}");
    assert_eq!(from_packed.as_ref(), Ok(expected), "MessagePack of {json}");
}

/// Checks that `json` fails to decode as `T` from JSON and from its `MessagePack` form.
pub fn fails_in_both<T: DeserializeOwned + Debug>(json: &str) {
    let (from_json, from_packed) = decode_both::<T>(json);
    assert!(from_json.is_err(), "JSON {json} decoded to {from_json:?}");
    assert!(
        from_packed.is_err(),
        "MessagePack of {json} decoded to {from_packed:?}"
    );
}

/// An envelope around `event`, with fixed ids: event 40, thread 2, seq 7, command 41.
pub fn envelope(event: DomainEvent) -> Envelope {
    Envelope::new(
        event_id(40),
        StreamId::Thread(thread_id(2)),
        Seq::new(7).unwrap(),
        Timestamp::new(1_760_000_000_000).unwrap(),
        Some(command_id(41)),
        event,
    )
}
