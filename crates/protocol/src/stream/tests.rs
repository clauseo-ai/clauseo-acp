//! `StreamId` and `EntryId` text forms, in JSON and in `MessagePack`.

use std::str::FromStr;

use uuid::Uuid;

use crate::test_support::err_contains;
use crate::{EntryId, StreamId};

const V7: &str = "01900000-0000-7000-8000-00000000002a";

/// Every kind parses from its text, prints it back and round-trips in JSON and `MessagePack`,
/// so each row of the `view_entries!` table is covered without a list of its own here.
#[test]
fn every_entry_id_round_trips_as_text() {
    for kind in EntryId::KINDS {
        let text = format!("{kind}:{V7}");
        let id = EntryId::from_str(&text).unwrap();
        assert_eq!(id.kind(), *kind);
        assert_eq!(id.uuid(), Uuid::parse_str(V7).unwrap());
        assert_eq!(id.to_string(), text);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<EntryId>(&json).unwrap(), id);
        let packed = rmp_serde::to_vec_named(&id).unwrap();
        assert_eq!(packed, rmp_serde::to_vec_named(&text).unwrap());
        assert_eq!(rmp_serde::from_slice::<EntryId>(&packed).unwrap(), id);
    }
}

#[test]
fn bad_entry_ids_are_errors() {
    let v4 = V7.replacen('7', "4", 1);
    let cases = [
        (String::new(), "expected <kind>:<uuid>"),
        ("tool_call".to_owned(), "expected <kind>:<uuid>"),
        (format!("toolcall:{V7}"), "unknown prefix \"toolcall\""),
        (format!("step:{v4}"), "expected a v7 UUID, got version 4"),
    ];
    for (bad, expected) in cases {
        err_contains(EntryId::from_str(&bad), expected);
    }
    err_contains(
        serde_json::from_str::<EntryId>("{\"step\":1}"),
        "expected an entry id",
    );
}

#[test]
fn escaped_json_text_still_parses() {
    // serde_json hands an escaped string to `visit_str` as a copy; the result is the same.
    let json = format!("\"thread\\u003a{V7}\"");
    let stream = serde_json::from_str::<StreamId>(&json).unwrap();
    assert_eq!(stream.kind(), "thread");
}
