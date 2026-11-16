//! acp: the scanner that reads the start of a `session/update` line over the 8 MiB line cap
//! (crates/acp/src/line/oversize.rs), called directly. `acp_stdout` cannot reach it: its
//! inputs are at most 64 KiB, and the reader scans a line only once it is past the cap.
//!
//! Items driven: `clauseo_acp::fuzzing::skipped_update`, the door to the scanner that
//! clauseo-acp's `fuzzing` feature opens, and the marker line it builds.
//!
//! Input: the start of the line, as the reader holds it.
//!
//! Invariants: no panic, and a read in linear time (a hang shows as a libFuzzer timeout). When
//! the start reads as skippable: each copied id is at most `MAX_ID_BYTES` of UTF-8 with no
//! control character; the status is one of ACP's four; the marker is one JSON object naming
//! `_clauseo/skipped_update` whose params repeat what was read, `null` for what was not.
//!
//! Against serde_json, when the input is one whole JSON value with no key repeated in any
//! object: each field the scanner read (`params.sessionId`, and `params.update`'s
//! `sessionUpdate`, `toolCallId` and `status`) is what serde_json reads there. When the input
//! also has no backslash, the update no `content` key, and each of those four fields is a
//! string (the ids at most `MAX_ID_BYTES`, the status one of the four), nothing stops the
//! scanner, so it reads every one of them that is there.
#![no_main]

use std::collections::HashSet;
use std::fmt;

use clauseo_acp::fuzzing::{MAX_ID_BYTES, SkippedUpdate, skipped_update};
use libfuzzer_sys::fuzz_target;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};

const STATUSES: [&str; 4] = ["pending", "in_progress", "completed", "failed"];

/// Any JSON value with no key repeated in any of its objects, its keys compared decoded.
struct NoRepeatedKeys;

impl<'de> Deserialize<'de> for NoRepeatedKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NoRepeatedKeys)
    }
}

impl<'de> Visitor<'de> for NoRepeatedKeys {
    type Value = Self;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_str<E>(self, _: &str) -> Result<Self, E> {
        Ok(self)
    }
    fn visit_unit<E>(self) -> Result<Self, E> {
        Ok(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self, A::Error> {
        while seq.next_element::<NoRepeatedKeys>()?.is_some() {}
        Ok(self)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self, A::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(de::Error::custom("a repeated key"));
            }
            map.next_value::<NoRepeatedKeys>()?;
        }
        Ok(self)
    }
}

fn check_id(id: Option<&String>) {
    if let Some(id) = id {
        assert!(id.len() <= MAX_ID_BYTES, "an id of {} bytes", id.len());
        assert!(
            id.bytes().all(|byte| byte >= 0x20),
            "a control character in {id:?}"
        );
    }
}

/// `field` of `object` when `object` is a JSON object.
fn field<'a>(object: Option<&'a Value>, field: &str) -> Option<&'a Value> {
    object
        .and_then(Value::as_object)
        .and_then(|map| map.get(field))
}

/// Whether nothing in `update`'s and `params`' read fields stops the scanner (see the header).
fn readable(params: &Map<String, Value>, update: Option<&Value>) -> bool {
    let id = |value: Option<&Value>| {
        value.is_none_or(|value| value.as_str().is_some_and(|id| id.len() <= MAX_ID_BYTES))
    };
    let update_ok = match update {
        None => true,
        Some(Value::Object(update)) => {
            !update.contains_key("content")
                && id(update.get("toolCallId"))
                && update.get("sessionUpdate").is_none_or(Value::is_string)
                && update
                    .get("status")
                    .is_none_or(|status| status.as_str().is_some_and(|s| STATUSES.contains(&s)))
        }
        Some(_) => false,
    };
    update_ok && id(params.get("sessionId"))
}

/// The scanner's read against serde_json's parse of the same whole line.
fn compare(head: &[u8], read: &SkippedUpdate) {
    if serde_json::from_slice::<NoRepeatedKeys>(head).is_err() {
        return;
    }
    let line: Value = serde_json::from_slice(head).expect("parsed once already");
    let params = field(Some(&line), "params");
    let update = field(params, "update");
    let session = field(params, "sessionId").and_then(Value::as_str);
    let kind = field(update, "sessionUpdate");
    let tool_call = field(update, "toolCallId").and_then(Value::as_str);
    let status = field(update, "status").and_then(Value::as_str);

    if let Some(read) = &read.session {
        assert_eq!(Some(read.as_str()), session, "sessionId");
    }
    if let Some(read) = &read.tool_call {
        assert_eq!(Some(read.as_str()), tool_call, "toolCallId");
    }
    if let Some(read) = read.status {
        assert_eq!(Some(read), status, "status");
    }
    if let Some(tool) = read.tool_call_update {
        let serde = kind.and_then(Value::as_str);
        assert_eq!(
            serde.map(|kind| kind == "tool_call_update"),
            Some(tool),
            "sessionUpdate"
        );
    }

    let Some(params) = params.and_then(Value::as_object) else {
        return;
    };
    if head.contains(&b'\\') || !readable(params, update) {
        return;
    }
    let expected_kind = kind
        .and_then(Value::as_str)
        .map(|kind| kind == "tool_call_update");
    assert_eq!(read.session.as_deref(), session, "sessionId, all read");
    assert_eq!(read.tool_call.as_deref(), tool_call, "toolCallId, all read");
    assert_eq!(read.status, status, "status, all read");
    assert_eq!(
        read.tool_call_update, expected_kind,
        "sessionUpdate, all read"
    );
}

fuzz_target!(|head: &[u8]| {
    let Some(read) = skipped_update(head) else {
        return;
    };
    check_id(read.session.as_ref());
    check_id(read.tool_call.as_ref());
    if let Some(status) = read.status {
        assert!(STATUSES.contains(&status), "{status}");
    }
    let marker: Value = serde_json::from_str(&read.marker).expect("the marker is JSON");
    let kind = read
        .tool_call_update
        .map(|tool| if tool { "tool_call_update" } else { "other" });
    let expected = json!({
        "jsonrpc": "2.0",
        "method": "_clauseo/skipped_update",
        "params": {
            "bytes": 9 * 1024 * 1024,
            "sessionId": read.session,
            "kind": kind,
            "toolCallId": read.tool_call,
            "status": read.status,
        },
    });
    assert_eq!(marker, expected);
    compare(head, &read);
});
