//! Rules the encoder and decoder enforce beyond the shape: 64-bit numbers stay at most 2^53 - 1,
//! an `Unknown` never takes a known kind, the envelope's version check, the stream rule, and the
//! conversions into `DomainEvent`.

use std::fmt::Debug;

use clauseo_protocol::{
    BlobHash, BlobRange, ChangeKind, ContentBlock, ContextUsage, DetachReason, DomainEvent,
    FileChange, HostQuery, ImportSummary, LimitSource, MAX_SAFE, SCHEMA_VERSION, SteerOutcome,
    TurnStarted, TurnUsage, UsageUpdated, clamp_safe,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::examples::{every_event, tool_call_started};
use crate::fixtures::{checkpoint_id, envelope, fails_in_both, path, raw, round_trips, turn_id};

/// Checks a type with a 64-bit field that `make` fills: 2^53 - 1 encodes and decodes in JSON and
/// `MessagePack`; 2^53 and `u64::MAX` fail to encode, and fail to decode in both formats.
fn capped<T, F>(make: F)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
    F: Fn(u64) -> T,
{
    let max = make(MAX_SAFE);
    round_trips(&max);
    let json = serde_json::to_string(&max).unwrap();
    let max_text = MAX_SAFE.to_string();
    assert!(json.contains(&max_text), "{json}");
    for over in [MAX_SAFE + 1, u64::MAX] {
        let value = make(over);
        let error = serde_json::to_string(&value).unwrap_err().to_string();
        assert!(error.contains("2^53 - 1"), "{error}");
        assert!(rmp_serde::to_vec_named(&value).is_err(), "{value:?}");
        // Same JSON with the larger number: refused from JSON and from its MessagePack form.
        fails_in_both::<T>(&json.replace(&max_text, &over.to_string()));
    }
}

#[test]
fn u64_fields_refuse_values_above_max_safe() {
    capped(|n| UsageUpdated {
        context_used: Some(n),
        context_size: None,
        cost: None,
    });
    capped(|n| UsageUpdated {
        context_used: None,
        context_size: Some(n),
        cost: None,
    });
    capped(|n| TurnUsage {
        total: n,
        ..TurnUsage::default()
    });
    capped(|n| TurnUsage {
        cached_write: Some(n),
        ..TurnUsage::default()
    });
    capped(|n| FileChange {
        path: path("a.docx"),
        change: ChangeKind::Added,
        before: None,
        after: None,
        size_bytes: n,
        may_include_lawyer_edits: false,
    });
    capped(|n| ImportSummary {
        files: 1,
        bytes: n,
        cloud_only: 0,
    });
    capped(|n| ContentBlock::ResourceLink {
        uri: "file:///a.pdf".to_owned(),
        name: "a.pdf".to_owned(),
        mime_type: None,
        title: None,
        description: None,
        size: Some(n),
    });
    capped(|n| BlobRange {
        offset: 0,
        end: n,
        text: String::new(),
    });
    capped(|n| HostQuery::ReadBlob {
        hash: BlobHash::from_bytes([7; 32]),
        offset: n,
        len: 1,
    });
    capped(|n| ContextUsage {
        context_used: Some(n),
        ..ContextUsage::default()
    });

    // `None` is still left out, and the plain form is a plain number.
    let none = UsageUpdated {
        context_used: None,
        context_size: Some(5),
        cost: None,
    };
    assert_eq!(
        serde_json::to_string(&none).unwrap(),
        r#"{"context_size":5}"#
    );
}

#[test]
fn clamp_safe_caps_at_max_safe() {
    assert_eq!(clamp_safe(0), 0);
    assert_eq!(clamp_safe(MAX_SAFE), MAX_SAFE);
    assert_eq!(clamp_safe(MAX_SAFE + 1), MAX_SAFE);
    assert_eq!(clamp_safe(u64::MAX), MAX_SAFE);
}

/// Checks that `value` fails to encode in both formats with an error naming `kind`.
fn refused<T: Serialize + Debug>(value: &T, kind: &str) {
    let error = serde_json::to_string(value).unwrap_err().to_string();
    assert!(error.contains(&format!("{kind:?}")), "{error}");
    let error = rmp_serde::to_vec_named(value).unwrap_err().to_string();
    assert!(error.contains(&format!("{kind:?}")), "{error}");
}

#[test]
fn unknown_under_a_known_kind_is_refused() {
    for kind in DomainEvent::KINDS {
        let unknown = DomainEvent::Unknown {
            kind: (*kind).to_owned(),
            payload: raw("{}"),
        };
        refused(&unknown, kind);
    }
    for kind in ["exited", "closed", "restarting", "idle_stopped"] {
        refused(
            &DetachReason::Unknown {
                kind: kind.to_owned(),
            },
            kind,
        );
    }
    for kind in [
        "fresh",
        "stale",
        "loading",
        "unavailable",
        "not_applicable",
        "live_only",
    ] {
        refused(
            &LimitSource::Unknown {
                kind: kind.to_owned(),
            },
            kind,
        );
    }
    for kind in ["injected", "requeued", "sent_now"] {
        refused(
            &SteerOutcome::Unknown {
                kind: kind.to_owned(),
            },
            kind,
        );
    }
    // A kind no version of this type knows still writes back.
    let later = SteerOutcome::Unknown {
        kind: "later".to_owned(),
    };
    assert_eq!(serde_json::to_string(&later).unwrap(), r#""later""#);
}

#[test]
fn envelope_newer_than_this_version_is_not_supported() {
    let current = envelope(
        TurnStarted {
            turn: turn_id(3),
            checkpoint_before: checkpoint_id(8),
        }
        .into(),
    );
    assert!(current.is_supported());
    let mut older = current.clone();
    older.schema_version = 0;
    assert!(older.is_supported());

    // A newer envelope still decodes, so the reader can see its version and stop.
    let json = serde_json::to_string(&current).unwrap().replace(
        r#""schema_version":1"#,
        &format!(r#""schema_version":{}"#, SCHEMA_VERSION + 1),
    );
    let newer: clauseo_protocol::Envelope = serde_json::from_str(&json).unwrap();
    assert_eq!(newer.schema_version, SCHEMA_VERSION + 1);
    assert!(!newer.is_supported());
}

#[test]
fn matter_level_kinds_are_the_four_matter_events() {
    let events = every_event();
    let matter: Vec<&str> = events
        .iter()
        .filter(|event| event.is_matter_level())
        .map(DomainEvent::kind)
        .collect();
    let expected = [
        "matter_created",
        "checkpoint_recorded",
        "outside_edits",
        "version_restored",
    ];
    assert_eq!(matter, expected);
    let unknown = DomainEvent::Unknown {
        kind: "later_matter_kind".to_owned(),
        payload: raw("{}"),
    };
    assert!(!unknown.is_matter_level());
}

#[test]
fn boxed_variants_convert_from_the_bare_payload() {
    let started = tool_call_started();
    assert_eq!(
        DomainEvent::from(started.clone()),
        DomainEvent::ToolCallStarted(Box::new(started.clone()))
    );
    assert_eq!(
        DomainEvent::from(Box::new(started.clone())),
        DomainEvent::from(started)
    );
    for event in every_event() {
        if let DomainEvent::ContentAppended(boxed) = &event {
            assert_eq!(DomainEvent::from((**boxed).clone()), event);
        }
        if let DomainEvent::ToolCallUpdated(boxed) = &event {
            assert_eq!(DomainEvent::from((**boxed).clone()), event);
        }
    }
}
