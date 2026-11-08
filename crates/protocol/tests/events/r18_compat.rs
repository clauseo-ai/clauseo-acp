//! R18's logged values as an older or newer reader meets them: each open enum's known values
//! keep their wire form and a newer one decodes to `Unknown`; a newer field inside a known
//! payload or variant is skipped; and to a reader before R18 its kinds are `Unknown` events,
//! which the fold skips (the reducer's `denials.rs` tests).

use clauseo_protocol::{
    AllowTarget, DenialKind, DenialRecorded, DenialSource, DomainEvent, EscalationAnswer,
    GrantMode, OtherMatter, ProtectionSwitched, ReadGrantedTo,
};

use super::unknown_variants::{data_enum_unknown, unit_enum};
use crate::fixtures::{
    allowance_id, decodes_in_both, denial_id, matter_id, round_trips, tool_call_id, turn_id,
    unknown_of, v7_text,
};

#[test]
fn escalation_answer() {
    use EscalationAnswer::{AllowForMatter, AllowOnce, Refuse, RefuseForMatter};
    let cases = [
        (AllowOnce, "allow_once"),
        (AllowForMatter, "allow_for_matter"),
        (Refuse, "refuse"),
        (RefuseForMatter, "refuse_for_matter"),
    ];
    unit_enum(&EscalationAnswer::ALL, &cases, EscalationAnswer::Unknown);
}

#[test]
fn grant_mode() {
    unit_enum(
        &GrantMode::ALL,
        &[(GrantMode::ReadOnly, "read_only")],
        GrantMode::Unknown,
    );
}

#[test]
fn denial_kind() {
    let cases = [
        (
            DenialKind::Folder {
                path: "/Users/a/Deals".to_owned(),
                other_matter: None,
            },
            r#"{"folder":{"path":"/Users/a/Deals"}}"#.to_owned(),
        ),
        (
            DenialKind::File {
                path: "/Users/a/Smith/brief.docx".to_owned(),
                other_matter: Some(OtherMatter {
                    matter: matter_id(3),
                    name: "Smith".to_owned(),
                }),
            },
            format!(
                r#"{{"file":{{"path":"/Users/a/Smith/brief.docx","other_matter":{{"matter":"{}","name":"Smith"}}}}}}"#,
                v7_text(3)
            ),
        ),
        (
            DenialKind::Site {
                host: "example.com".to_owned(),
                shared_hosting: false,
            },
            r#"{"site":{"host":"example.com","shared_hosting":false}}"#.to_owned(),
        ),
        (
            DenialKind::UpstreamAuth {
                upstream: "proxy.firm.example".to_owned(),
            },
            r#"{"upstream_auth":{"upstream":"proxy.firm.example"}}"#.to_owned(),
        ),
    ];
    for (value, json) in &cases {
        assert_eq!(&serde_json::to_string(value).unwrap(), json);
        round_trips(value);
    }
    data_enum_unknown(&unknown_of::<DenialKind>("later"));
}

#[test]
fn denial_source() {
    let tool = DenialSource::ToolCall {
        tool_call: tool_call_id(4),
    };
    let cases = [
        (DenialSource::Sandbox, r#""sandbox""#.to_owned()),
        (DenialSource::Proxy, r#""proxy""#.to_owned()),
        (
            tool,
            format!(r#"{{"tool_call":{{"tool_call":"{}"}}}}"#, v7_text(4)),
        ),
    ];
    for (value, json) in &cases {
        assert_eq!(&serde_json::to_string(value).unwrap(), json);
        round_trips(value);
    }
    data_enum_unknown(&unknown_of::<DenialSource>("later"));
}

#[test]
fn allow_target() {
    let cases = [
        AllowTarget::Folder {
            path: "/Users/a/Deals".to_owned(),
        },
        AllowTarget::File {
            path: "/Users/a/notes.docx".to_owned(),
        },
        AllowTarget::OtherMatterPath {
            matter: matter_id(3),
            path: "/Users/a/Smith".to_owned(),
            is_file: false,
        },
        AllowTarget::Site {
            host: "example.com".to_owned(),
        },
    ];
    for value in &cases {
        round_trips(value);
    }
    data_enum_unknown(&unknown_of::<AllowTarget>("later"));
}

/// Reads `json` as `T` and checks it is `expected`. JSON only: an id is 16 bytes in
/// `MessagePack`, so these forms are not `MessagePack` ones (the corpus pins those).
fn reads<T: serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(json: &str, expected: &T) {
    assert_eq!(
        &serde_json::from_str::<T>(json).unwrap(),
        expected,
        "{json}"
    );
}

/// A newer field beside a payload's own, or inside a known kind, is skipped; a field left out
/// that has a default reads as its default.
#[test]
fn a_newer_field_is_skipped() {
    let denial = v7_text(1);
    let turn = v7_text(3);
    let json = format!(
        r#"{{"turn":"{turn}","denial":"{denial}","kind":{{"site":{{"host":"example.com","port":443}}}},"source":"proxy","offered":true,"count":1,"why":"later"}}"#
    );
    reads(
        &json,
        &DenialRecorded {
            turn: Some(turn_id(3)),
            denial: denial_id(1),
            kind: DenialKind::Site {
                host: "example.com".to_owned(),
                shared_hosting: false,
            },
            source: DenialSource::Proxy,
            offered: true,
            count: 1,
        },
    );
    decodes_in_both(
        r#"{"off":true,"by":"policy"}"#,
        &ProtectionSwitched { off: true },
    );
    let json = format!(
        r#"{{"by_matter":"{}","by_name":"Jones","allowance":"{}","until":5}}"#,
        v7_text(4),
        v7_text(2)
    );
    reads(
        &json,
        &ReadGrantedTo {
            by_matter: matter_id(4),
            by_name: "Jones".to_owned(),
            allowance: allowance_id(2),
        },
    );
}

/// A newer value in a known kind's enum field decodes to its `Unknown`, so the event still reads.
#[test]
fn a_newer_value_inside_a_known_kind_reads_as_unknown() {
    let json = format!(
        r#"{{"denial_recorded":{{"denial":"{}","kind":{{"device":{{"name":"usb"}}}},"source":"kernel","offered":false,"count":1}}}}"#,
        v7_text(1)
    );
    let event: DomainEvent = serde_json::from_str(&json).unwrap();
    let DomainEvent::DenialRecorded(recorded) = event else {
        panic!("not a denial: {event:?}");
    };
    assert_eq!(recorded.kind, unknown_of::<DenialKind>("device"));
    assert_eq!(recorded.source, unknown_of::<DenialSource>("kernel"));
    assert_eq!(recorded.turn, None);
}

/// A kind after R18 (a newer reader's) is an `Unknown` event to this one, as R18's are to a
/// reader before it.
#[test]
fn a_newer_kind_reads_as_an_unknown_event() {
    let json = r#"{"denial_widened":{"denial":"x","to":"/"}}"#;
    let event: DomainEvent = serde_json::from_str(json).unwrap();
    assert!(
        matches!(&event, DomainEvent::Unknown { kind, .. } if kind == "denial_widened"),
        "{event:?}"
    );
}
