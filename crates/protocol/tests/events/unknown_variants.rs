//! One test per enum that decodes values a newer Clauseo may add: each known value keeps its
//! wire form in JSON and `MessagePack`, and an unknown one decodes to the catch-all instead of
//! failing. Malformed input still fails.

use std::fmt::Debug;

use clauseo_protocol::{
    CheckpointKind, ContentBlock, Deliverable, Delivery, DetachReason, DomainEvent, ErrorCode,
    ExpiryReason, HostError, LimitSource, NoticeLevel, PermissionKind, RawFrom, RenameSource,
    SkippedFileReason, StaleReason, SteerOutcome, StopReason, TextOrBlob, Timestamp,
    ToolCallStatus, ToolKind, UnavailableReason,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::examples::acp_unsupported_block;
use crate::fixtures::{
    decodes_in_both, fails_in_both, msgpack_of, raw, round_trips, turn_id, v7_text,
};

/// Checks a unit-only enum: `cases` pairs every variant but `Unknown` with its wire string, in
/// `ALL` order; `all` is the enum's `ALL`; `unknown` its catch-all.
fn unit_enum<T>(all: &[T], cases: &[(T, &str)], unknown: T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug + Copy,
{
    let mut listed: Vec<T> = cases.iter().map(|(value, _)| *value).collect();
    listed.push(unknown);
    assert_eq!(listed, all, "cases follow ALL, Unknown last");
    for (value, text) in cases {
        let json = format!("\"{text}\"");
        assert_eq!(serde_json::to_string(value).unwrap(), json);
        assert_eq!(rmp_serde::to_vec_named(value).unwrap(), msgpack_of(&json));
        decodes_in_both(&json, value);
    }
    decodes_in_both(r#""from_a_newer_clauseo""#, &unknown);
    assert_eq!(serde_json::to_string(&unknown).unwrap(), r#""unknown""#);
    round_trips(&unknown);
    for bad in ["null", r#"{"from_a_newer_clauseo":{"x":1}}"#, "[]"] {
        fails_in_both::<T>(bad);
    }
    // A number is no variant in JSON. rmp-serde reads it as a variant index, as for every
    // derived enum; Clauseo writes names (`to_vec_named`), never indices.
    assert!(serde_json::from_str::<T>("1").is_err());
    let by_index: T = rmp_serde::from_slice(&msgpack_of("1")).unwrap();
    assert_eq!(by_index, all[1]);
}

#[test]
fn tool_kind() {
    use ToolKind::{Delete, Edit, Execute, Fetch, Move, Other, Read, Search, SwitchMode, Think};
    let cases = [
        (Read, "read"),
        (Edit, "edit"),
        (Delete, "delete"),
        (Move, "move"),
        (Search, "search"),
        (Execute, "execute"),
        (Think, "think"),
        (Fetch, "fetch"),
        (SwitchMode, "switch_mode"),
        (Other, "other"),
    ];
    unit_enum(&ToolKind::ALL, &cases, ToolKind::Unknown);
}

#[test]
fn tool_call_status() {
    use ToolCallStatus::{Cancelled, Completed, Failed, InProgress, Pending};
    let cases = [
        (Pending, "pending"),
        (InProgress, "in_progress"),
        (Completed, "completed"),
        (Failed, "failed"),
        (Cancelled, "cancelled"),
    ];
    unit_enum(&ToolCallStatus::ALL, &cases, ToolCallStatus::Unknown);
}

#[test]
fn permission_kind() {
    use PermissionKind::{AllowAlways, AllowOnce, RejectAlways, RejectOnce};
    let cases = [
        (AllowOnce, "allow_once"),
        (AllowAlways, "allow_always"),
        (RejectOnce, "reject_once"),
        (RejectAlways, "reject_always"),
    ];
    unit_enum(&PermissionKind::ALL, &cases, PermissionKind::Unknown);
}

#[test]
fn stop_reason() {
    use StopReason::{AtLimit, Cancelled, EndTurn, Failed, MaxTokens, MaxTurnRequests, Refusal};
    let cases = [
        (EndTurn, "end_turn"),
        (MaxTokens, "max_tokens"),
        (MaxTurnRequests, "max_turn_requests"),
        (Refusal, "refusal"),
        (Cancelled, "cancelled"),
        (AtLimit, "at_limit"),
        (Failed, "failed"),
    ];
    unit_enum(&StopReason::ALL, &cases, StopReason::Unknown);
    let names: Vec<_> = StopReason::ALL
        .iter()
        .map(StopReason::variant_name)
        .collect();
    assert_eq!(names.first(), Some(&"EndTurn"));
    assert_eq!(names.last(), Some(&"Unknown"));
}

#[test]
fn notice_level() {
    let cases = [
        (NoticeLevel::Info, "info"),
        (NoticeLevel::Warning, "warning"),
    ];
    let all = [
        NoticeLevel::Info,
        NoticeLevel::Warning,
        NoticeLevel::Unknown,
    ];
    unit_enum(&all, &cases, NoticeLevel::Unknown);
}

#[test]
fn deliverable() {
    let cases = [
        (Deliverable::Live, "live"),
        (Deliverable::NotResumable, "not_resumable"),
    ];
    let all = [
        Deliverable::Live,
        Deliverable::NotResumable,
        Deliverable::Unknown,
    ];
    unit_enum(&all, &cases, Deliverable::Unknown);
}

#[test]
fn checkpoint_kind() {
    use CheckpointKind::{AfterTurn, BeforeTurn, Import, OutsideEdits, Restore, Undo};
    let cases = [
        (Import, "import"),
        (BeforeTurn, "before_turn"),
        (AfterTurn, "after_turn"),
        (OutsideEdits, "outside_edits"),
        (Undo, "undo"),
        (Restore, "restore"),
    ];
    unit_enum(&CheckpointKind::ALL, &cases, CheckpointKind::Unknown);
}

#[test]
fn expiry_reason() {
    use ExpiryReason::{AgentExited, HostRestarted, TurnEnded};
    let cases = [
        (AgentExited, "agent_exited"),
        (TurnEnded, "turn_ended"),
        (HostRestarted, "host_restarted"),
    ];
    unit_enum(&ExpiryReason::ALL, &cases, ExpiryReason::Unknown);
}

#[test]
fn rename_source() {
    let cases = [
        (RenameSource::Lawyer, "lawyer"),
        (RenameSource::Agent, "agent"),
    ];
    let all = [
        RenameSource::Lawyer,
        RenameSource::Agent,
        RenameSource::Unknown,
    ];
    unit_enum(&all, &cases, RenameSource::Unknown);
}

#[test]
fn delivery() {
    let cases = [
        (Delivery::Queue, "queue"),
        (Delivery::SendNow, "send_now"),
        (Delivery::Steer, "steer"),
    ];
    unit_enum(&Delivery::ALL, &cases, Delivery::Unknown);
}

#[test]
fn skipped_file_reason() {
    let cases = [
        (SkippedFileReason::OverSizeCap, "over_size_cap"),
        (SkippedFileReason::CloudOnly, "cloud_only"),
    ];
    unit_enum(&SkippedFileReason::ALL, &cases, SkippedFileReason::Unknown);
}

/// Checks the catch-all of an enum with data: an unknown string and an unknown key with a
/// payload decode to `unknown`, which writes back as the plain string.
fn data_enum_unknown<T>(unknown: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    decodes_in_both(r#""later""#, unknown);
    decodes_in_both(r#"{"later":{"b":[1,{"c":null}],"a":"x"}}"#, unknown);
    decodes_in_both(r#"{"later":null}"#, unknown);
    assert_eq!(serde_json::to_string(unknown).unwrap(), r#""later""#);
    round_trips(unknown);
    for bad in ["{}", r#"{"later":1,"other":2}"#, "3", "[]"] {
        fails_in_both::<T>(bad);
    }
}

#[test]
fn detach_reason() {
    let exited = DetachReason::Exited {
        code: Some(-1),
        stderr_tail: "panic".to_owned(),
    };
    let signalled = DetachReason::Exited {
        code: None,
        stderr_tail: String::new(),
    };
    let cases = [
        (exited, r#"{"exited":{"code":-1,"stderr_tail":"panic"}}"#),
        (signalled, r#"{"exited":{"stderr_tail":""}}"#),
        (DetachReason::Closed, r#""closed""#),
        (DetachReason::Restarting, r#""restarting""#),
        (DetachReason::IdleStopped, r#""idle_stopped""#),
    ];
    for (value, json) in &cases {
        assert_eq!(serde_json::to_string(value).unwrap(), *json);
        assert_eq!(rmp_serde::to_vec_named(value).unwrap(), msgpack_of(json));
        decodes_in_both(json, value);
    }
    data_enum_unknown(&DetachReason::Unknown {
        kind: "later".to_owned(),
    });
    fails_in_both::<DetachReason>(r#""exited""#);
    fails_in_both::<DetachReason>(r#"{"exited":{"code":"x","stderr_tail":""}}"#);
    let names: Vec<_> = DetachReason::ALL
        .iter()
        .map(DetachReason::variant_name)
        .collect();
    assert_eq!(
        names,
        ["Exited", "Closed", "Restarting", "IdleStopped", "Unknown"]
    );
}

#[test]
fn limit_source() {
    let at = Timestamp::new(1_760_000_000_000).unwrap();
    let cases = [
        (
            LimitSource::Fresh { checked_at: at },
            r#"{"fresh":{"checked_at":1760000000000}}"#,
        ),
        (
            LimitSource::Stale {
                checked_at: at,
                reason: StaleReason::ReadFailed,
            },
            r#"{"stale":{"checked_at":1760000000000,"reason":"read_failed"}}"#,
        ),
        (LimitSource::Loading, r#""loading""#),
        (
            LimitSource::Unavailable {
                reason: UnavailableReason::MissingScope,
            },
            r#"{"unavailable":{"reason":"missing_scope"}}"#,
        ),
        (LimitSource::NotApplicable, r#""not_applicable""#),
        (
            LimitSource::LiveOnly { updated_at: at },
            r#"{"live_only":{"updated_at":1760000000000}}"#,
        ),
    ];
    for (value, json) in &cases {
        assert_eq!(serde_json::to_string(value).unwrap(), *json);
        assert_eq!(rmp_serde::to_vec_named(value).unwrap(), msgpack_of(json));
        decodes_in_both(json, value);
    }
    data_enum_unknown(&LimitSource::Unknown {
        kind: "later".to_owned(),
    });
    fails_in_both::<LimitSource>(r#""fresh""#);
    fails_in_both::<LimitSource>(r#"{"unavailable":{"reason":"gone"}}"#);
    fails_in_both::<LimitSource>(r#"{"fresh":{"checked_at":9007199254740993}}"#);

    let names: Vec<_> = LimitSource::ALL
        .iter()
        .map(LimitSource::variant_name)
        .collect();
    let expected = [
        "Fresh",
        "Stale",
        "Loading",
        "Unavailable",
        "NotApplicable",
        "LiveOnly",
        "Unknown",
    ];
    assert_eq!(names, expected);
}

#[test]
fn steer_outcome() {
    let turn = turn_id(7);
    let cases = [
        (SteerOutcome::Injected { turn }, "injected"),
        (SteerOutcome::Requeued { turn }, "requeued"),
        (SteerOutcome::SentNow { turn }, "sent_now"),
    ];
    for (value, key) in &cases {
        let json = format!(r#"{{"{key}":{{"turn":"{}"}}}}"#, v7_text(7));
        assert_eq!(serde_json::to_string(value).unwrap(), json);
        assert_eq!(serde_json::from_str::<SteerOutcome>(&json).unwrap(), *value);
        round_trips(value);
    }
    data_enum_unknown(&SteerOutcome::Unknown {
        kind: "later".to_owned(),
    });
    fails_in_both::<SteerOutcome>(r#""injected""#);
    fails_in_both::<SteerOutcome>(r#"{"injected":{"turn":"not-a-uuid"}}"#);
    let names: Vec<_> = SteerOutcome::ALL
        .iter()
        .map(SteerOutcome::variant_name)
        .collect();
    assert_eq!(names, ["Injected", "Requeued", "SentNow", "Unknown"]);
}

#[test]
fn content_block() {
    // A block kind from a newer Clauseo keeps the whole one-key object as compact JSON.
    let later = r#"{"hologram":{"z":1,"a":[1,{"b":null}],"s":"\"q\""}}"#;
    let expected = ContentBlock::Unsupported {
        kind: "hologram".to_owned(),
        raw_from: RawFrom::Clauseo,
        raw: Some(TextOrBlob::Inline(later.to_owned())),
    };
    decodes_in_both(later, &expected);
    let written = serde_json::to_string(&expected).unwrap();
    let kept = serde_json::to_string(later).unwrap();
    let wire = format!(
        r#"{{"unsupported":{{"kind":"hologram","raw_from":"clauseo","raw":{{"inline":{kept}}}}}}}"#
    );
    assert_eq!(written, wire);
    round_trips(&expected);
    // A plain string from a newer Clauseo keeps the string's JSON.
    let unit = ContentBlock::Unsupported {
        kind: "later".to_owned(),
        raw_from: RawFrom::Clauseo,
        raw: Some(TextOrBlob::Inline(r#""later""#.to_owned())),
    };
    decodes_in_both(r#""later""#, &unit);

    // Every known kind round-trips, `acp`'s own Unsupported included.
    let blocks = [
        r#"{"text":{"text":"hi"}}"#,
        r#"{"image":{"data":{"inline":"iVBO"},"mime_type":"image/png","uri":"file:///a.png"}}"#,
        r#"{"audio":{"data":{"inline":"UklG"},"mime_type":"audio/wav"}}"#,
        r#"{"resource_link":{"uri":"file:///a.pdf","name":"a.pdf","size":10}}"#,
        r#"{"resource":{"uri":"file:///a.md","mime_type":"text/markdown","text":{"inline":"x"}}}"#,
    ];
    for json in blocks {
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(!matches!(block, ContentBlock::Unsupported { .. }), "{json}");
        assert_eq!(serde_json::to_string(&block).unwrap(), json);
        assert_eq!(rmp_serde::to_vec_named(&block).unwrap(), msgpack_of(json));
        decodes_in_both(json, &block);
    }
    round_trips(&acp_unsupported_block());

    // A known kind with a bad payload is an error, not Unsupported.
    fails_in_both::<ContentBlock>(r#"{"image":{"data":3,"mime_type":"image/png"}}"#);
    fails_in_both::<ContentBlock>(r#"{"text":{}}"#);
    fails_in_both::<ContentBlock>(r#""text""#);
    fails_in_both::<ContentBlock>(r#"{"text":{"text":"a"},"image":{}}"#);
    fails_in_both::<ContentBlock>("{}");
}

#[test]
fn domain_event() {
    let later = r#"{"later_kind":{"b":2,"a":1}}"#;
    let expected = DomainEvent::Unknown {
        kind: "later_kind".to_owned(),
        payload: raw(r#"{"b":2,"a":1}"#),
    };
    decodes_in_both(later, &expected);
    assert_eq!(serde_json::to_string(&expected).unwrap(), later);
    assert_eq!(
        rmp_serde::to_vec_named(&expected).unwrap(),
        msgpack_of(later)
    );
    round_trips(&expected);
    fails_in_both::<DomainEvent>(r#"{"turn_started":{"turn":"x"}}"#);
    fails_in_both::<DomainEvent>(r#"{"later_kind":1,"other_kind":2}"#);
}

#[test]
fn error_code() {
    let disk_full = serde_json::from_str::<ErrorCode>(r#""store.disk_full""#).unwrap();
    assert_eq!(disk_full, ErrorCode::StoreDiskFull);
    let later = serde_json::from_str::<ErrorCode>(r#""store.on_fire""#).unwrap();
    assert_eq!(later, ErrorCode::Unknown);
    assert_eq!(ErrorCode::Unknown.as_str(), "unknown");
    assert_eq!(
        serde_json::to_string(&ErrorCode::Unknown).unwrap(),
        r#""unknown""#
    );
    let packed = rmp_serde::to_vec_named("store.on_fire").unwrap();
    assert_eq!(
        rmp_serde::from_slice::<ErrorCode>(&packed).unwrap(),
        ErrorCode::Unknown
    );
    fails_in_both::<ErrorCode>("3");

    // HostError keeps the unknown code in its message.
    let json = r#"{"code":"store.on_fire","message":"smoke","retryable":true}"#;
    let error: HostError = serde_json::from_str(json).unwrap();
    let message = "store.on_fire: smoke".to_owned();
    assert_eq!(
        error,
        HostError {
            code: ErrorCode::Unknown,
            message,
            retryable: true
        }
    );
    let error: HostError = rmp_serde::from_slice(&msgpack_of(json)).unwrap();
    assert_eq!(error.message, "store.on_fire: smoke");

    let known = r#"{"code":"thread.queue_full","message":"ten queued","retryable":true}"#;
    let error: HostError = serde_json::from_str(known).unwrap();
    assert_eq!(
        error,
        HostError::new(ErrorCode::ThreadQueueFull, "ten queued")
    );
    assert_eq!(serde_json::to_string(&error).unwrap(), known);
}
