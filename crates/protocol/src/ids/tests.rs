//! Ids and small values: constructors and `Deserialize` apply the same rules.

use std::str::FromStr;

use uuid::Uuid;

use crate::test_support::err_contains;
use crate::{
    AgentId, AgentSessionId, BlobHash, CheckpointId, CommandId, ContentId, Cursor, DomainEvent,
    ElicitationId, EventId, FileRefId, FolderRefId, MAX_SAFE, MatterId, MatterPath, MessageId,
    PermissionRequestId, Seq, SteerOutcome, ThreadId, ThreadStatus, Timestamp, ToolCallId, TurnId,
    TurnStarted, ViewEpoch,
};

const V7: &str = "01900000-0000-7000-8000-000000000001";
const V4: &str = "01900000-0000-4000-8000-000000000001";
const NIL: &str = "00000000-0000-0000-0000-000000000000";

fn json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| e.to_string())
}

#[test]
fn clauseo_ids_must_be_v7() {
    let v7 = Uuid::parse_str(V7).unwrap();
    let id = ThreadId::new(v7).unwrap();
    assert_eq!(id.as_uuid(), v7);
    assert_eq!(id.to_string(), V7);
    assert_eq!(ThreadId::from_str(V7).unwrap(), id);
    err_contains(
        ThreadId::new(Uuid::parse_str(V4).unwrap()),
        "expected a v7 UUID, got version 4",
    );
    err_contains(
        ThreadId::new(Uuid::nil()),
        "expected a v7 UUID, got version 0",
    );
    err_contains(
        ThreadId::from_str(&V7.replace('-', "")),
        "not a hyphenated UUID",
    );

    assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{V7}\""));
    assert_eq!(json::<ThreadId>(&format!("\"{V7}\"")).unwrap(), id);
    assert!(
        json::<ThreadId>(&format!("\"{V4}\""))
            .unwrap_err()
            .contains("v7")
    );
}

#[test]
fn ids_are_16_bytes_in_messagepack_and_checked_there_too() {
    let id = ThreadId::from_str(V7).unwrap();
    let packed = rmp_serde::to_vec_named(&id).unwrap();
    assert_eq!(packed.len(), 18, "bin8 marker, length, 16 bytes");
    assert_eq!(rmp_serde::from_slice::<ThreadId>(&packed).unwrap(), id);
    let v4 = rmp_serde::to_vec_named(&Uuid::parse_str(V4).unwrap()).unwrap();
    err_contains(
        rmp_serde::from_slice::<ThreadId>(&v4),
        "expected a v7 UUID, got version 4",
    );
}

#[test]
fn command_ids_may_be_any_uuid_but_nil() {
    assert!(CommandId::from_str(V4).is_ok());
    assert!(CommandId::from_str(V7).is_ok());
    err_contains(CommandId::from_str(NIL), "the nil UUID is not an id");
    err_contains(
        json::<CommandId>(&format!("\"{NIL}\"")),
        "the nil UUID is not an id",
    );
}

#[test]
fn numbers_stay_below_two_to_the_53() {
    err_contains(Seq::new(0), "0 is outside 1..=9007199254740991");
    assert_eq!(Seq::new(1).unwrap(), Seq::FIRST);
    assert_eq!(Seq::new(MAX_SAFE).unwrap().get(), MAX_SAFE);
    err_contains(
        Seq::new(MAX_SAFE + 1),
        "9007199254740992 is outside 1..=9007199254740991",
    );
    err_contains(
        Seq::new(MAX_SAFE).unwrap().next(),
        "9007199254740992 is outside 1..=9007199254740991",
    );
    assert_eq!(Seq::FIRST.next().unwrap().get(), 2);
    assert_eq!(Cursor::new(0).unwrap(), Cursor::ZERO);
    err_contains(
        Cursor::new(MAX_SAFE + 1),
        "9007199254740992 is outside 0..=9007199254740991",
    );
    err_contains(json::<Seq>("0"), "0 is outside 1..=9007199254740991");
    err_contains(
        json::<Seq>("9007199254740992"),
        "9007199254740992 is outside 1..=9007199254740991",
    );
    assert_eq!(json::<Seq>("9007199254740991").unwrap().get(), MAX_SAFE);
    assert_eq!(serde_json::to_string(&Seq::FIRST).unwrap(), "1");

    let max = i64::try_from(MAX_SAFE).unwrap();
    assert!(Timestamp::new(-max).is_ok());
    err_contains(
        Timestamp::new(max + 1),
        "9007199254740992 is outside -9007199254740991..=9007199254740991",
    );
    err_contains(
        Timestamp::new(i64::MIN),
        "-9223372036854775808 is outside -9007199254740991..=9007199254740991",
    );
    assert_eq!(json::<Timestamp>("-5").unwrap().as_millis(), -5);
    err_contains(
        json::<Timestamp>("9007199254740992"),
        "9007199254740992 is outside -9007199254740991..=9007199254740991",
    );
}

#[test]
fn blob_hash_is_hex_in_json_and_bytes_in_messagepack() {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xab;
    bytes[31] = 0x01;
    let hash = BlobHash::from_bytes(bytes);
    let text = format!("ab{}01", "00".repeat(30));
    assert_eq!(hash.to_string(), text);
    assert_eq!(BlobHash::from_str(&text).unwrap(), hash);
    assert_eq!(serde_json::to_string(&hash).unwrap(), format!("\"{text}\""));
    assert_eq!(
        serde_json::from_str::<BlobHash>(&format!("\"{text}\"")).unwrap(),
        hash
    );
    // MessagePack writes the 32 bytes, bin 8 with length 32, and refuses the text form.
    let mut bin = vec![0xc4, 32];
    bin.extend_from_slice(&bytes);
    let packed = rmp_serde::to_vec_named(&hash).unwrap();
    assert_eq!(packed, bin);
    assert_eq!(rmp_serde::from_slice::<BlobHash>(&packed).unwrap(), hash);
    err_contains(
        rmp_serde::from_slice::<BlobHash>(&rmp_serde::to_vec(&text).unwrap()),
        "expected 32 bytes",
    );
    err_contains(
        serde_json::from_str::<BlobHash>(&format!("{bin:?}")),
        "expected a 64-character lowercase hex hash",
    );
    let short = rmp_serde::to_vec_named(&serde_bytes_of(&bytes[..31])).unwrap();
    let error = rmp_serde::from_slice::<BlobHash>(&short)
        .unwrap_err()
        .to_string();
    assert!(error.contains("expected 32 bytes"), "{error}");
    let long = rmp_serde::to_vec_named(&serde_bytes_of(&[7u8; 33])).unwrap();
    err_contains(
        rmp_serde::from_slice::<BlobHash>(&long),
        "invalid length 33, expected 32 bytes",
    );
    for bad in [
        text.to_uppercase(),
        text.get(..62).unwrap().to_owned(),
        format!("{text}00"),
        "zz".repeat(32),
    ] {
        err_contains(
            BlobHash::from_str(&bad),
            "expected 64 lowercase hex characters",
        );
    }
    err_contains(
        BlobHash::from_str(&format!("é{}", "0".repeat(62))),
        "expected 64 lowercase hex characters",
    );
}

#[test]
fn string_ids_check_their_format() {
    assert!(AgentId::new("claude-acp").is_ok());
    assert!(AgentId::new(&"a".repeat(64)).is_ok());
    for bad in ["", "Claude", "a_b", "a b", &"a".repeat(65)] {
        err_contains(
            AgentId::new(bad),
            "expected 1 to 64 characters of [a-z0-9-]",
        );
    }
    err_contains(
        json::<AgentId>("\"Codex\""),
        "expected 1 to 64 characters of [a-z0-9-]",
    );
    assert_eq!(
        json::<AgentId>("\"codex-acp\"").unwrap().as_str(),
        "codex-acp"
    );

    assert!(ContentId::new("0123abcdef").is_ok());
    err_contains(
        ContentId::new("ABC"),
        "expected 1 to 64 lowercase hex characters",
    );
    err_contains(
        ContentId::new(""),
        "expected 1 to 64 lowercase hex characters",
    );
    assert!(AgentSessionId::new("sess_01").is_ok());
    err_contains(AgentSessionId::new(""), "must not be empty");
}

#[test]
fn matter_paths_are_relative_and_clean() {
    for good in [
        "a",
        "a/b.docx",
        "Drafts/2026 v2/brief (final).docx",
        ".hidden/x",
        "a..b",
    ] {
        assert_eq!(MatterPath::new(good).unwrap().as_str(), good);
    }
    let relative = "must be relative, with no empty, `.` or `..` segment";
    let bad = [
        ("", "must not be empty"),
        ("/a", relative),
        ("a/", relative),
        ("a//b", relative),
        ("./a", relative),
        ("a/./b", relative),
        ("../a", relative),
        ("a/..", relative),
        ("a\\b", "must not contain a backslash"),
        ("a\0b", "must not contain a control character"),
    ];
    for (path, rule) in bad {
        err_contains(MatterPath::new(path), rule);
    }
    err_contains(json::<MatterPath>(r#""../etc/passwd""#), "must be relative");
}

#[test]
fn matter_paths_refuse_drives_controls_and_long_names() {
    for drive in ["C:", "C:x", "c:/Users/a.docx", "z:a/b"] {
        err_contains(
            MatterPath::new(drive),
            "must not start with a drive letter and `:`",
        );
    }
    // A `:` elsewhere is a legal macOS file name character.
    for good in ["ab:c", "notes/C:x", "1:/a", "x/a:b"] {
        assert!(MatterPath::new(good).is_ok(), "{good:?}");
    }
    for control in [
        "a\u{1}b", "a\tb", "a\nb", "a\u{1f}", "a\u{7f}b", "a\0b", "a\u{85}b", "a\u{9b}b", "\u{80}",
    ] {
        err_contains(
            MatterPath::new(control),
            "must not contain a control character",
        );
    }
    // Format characters stay allowed: real Hebrew and Arabic names hold bidi marks.
    for format in [
        "\u{5e9}\u{5dc}\u{5d5}\u{5dd}\u{200f}.docx",
        "a\u{200e}b",
        "a\u{a0}b",
        "a\u{202e}b",
    ] {
        assert!(MatterPath::new(format).is_ok(), "{format:?}");
    }
    let segment = "s".repeat(255);
    assert!(MatterPath::new(&segment).is_ok());
    assert!(MatterPath::new(&format!("a/{segment}")).is_ok());
    err_contains(
        MatterPath::new(&format!("{segment}s")),
        "has a segment longer than 255 bytes",
    );
    err_contains(
        MatterPath::new(&format!("a/{segment}s/b")),
        "has a segment longer than 255 bytes",
    );
    // 4096 bytes: 15 segments of 255 bytes, one of 254, then `/x`, with 16 slashes in all.
    let mut at_limit = vec![segment.as_str(); 15].join("/");
    at_limit.push('/');
    at_limit.push_str(&"s".repeat(254));
    at_limit.push_str("/x");
    assert_eq!(at_limit.len(), 4096);
    assert!(MatterPath::new(&at_limit).is_ok());
    at_limit.push('x');
    err_contains(MatterPath::new(&at_limit), "longer than 4096 bytes");
    err_contains(
        json::<MatterPath>(r#""C:/x""#),
        "must not start with a drive letter and `:`",
    );
    err_contains(
        json::<MatterPath>(r#""a\u0001""#),
        "must not contain a control character",
    );
}

#[test]
fn uuid_ids_decode_only_the_hyphenated_text_or_16_bytes() {
    let simple = V7.replace('-', "");
    let braced = format!("{{{V7}}}");
    let urn = format!("urn:uuid:{V7}");
    for form in [simple, braced, urn] {
        err_contains(
            json::<ThreadId>(&format!("\"{form}\"")),
            "not a hyphenated UUID",
        );
    }
    assert!(json::<ThreadId>(&format!("\"{V7}\"")).is_ok());
    // MessagePack: a str, or bin of another length, is refused.
    let as_str = rmp_serde::to_vec_named(V7).unwrap();
    err_contains(
        rmp_serde::from_slice::<ThreadId>(&as_str),
        "expected 16 bytes",
    );
    let id = ThreadId::from_str(V7).unwrap();
    let mut short =
        rmp_serde::to_vec_named(&serde_bytes_of(&id.as_uuid().as_bytes()[..15])).unwrap();
    err_contains(
        rmp_serde::from_slice::<ThreadId>(&short),
        "invalid length 15, expected 16 bytes",
    );
    short = rmp_serde::to_vec_named(&serde_bytes_of(&[7u8; 17])).unwrap();
    err_contains(
        rmp_serde::from_slice::<ThreadId>(&short),
        "invalid length 17, expected 16 bytes",
    );
    let packed = rmp_serde::to_vec_named(&id).unwrap();
    assert_eq!(rmp_serde::from_slice::<ThreadId>(&packed).unwrap(), id);
    // CommandId keeps its own rule: any version, never nil, strict forms alike.
    assert!(json::<CommandId>(&format!("\"{V4}\"")).is_ok());
    err_contains(
        json::<CommandId>(&format!("\"{}\"", V4.replace('-', ""))),
        "not a hyphenated UUID",
    );
    let nil = rmp_serde::to_vec_named(&Uuid::nil()).unwrap();
    err_contains(
        rmp_serde::from_slice::<CommandId>(&nil),
        "the nil UUID is not an id",
    );
}

/// Bytes that serialise as `MessagePack` bin.
fn serde_bytes_of(bytes: &[u8]) -> crate::RawPayload {
    crate::RawPayload::Bytes(bytes.into())
}

/// Encodes `value` in JSON and `MessagePack` and checks both decode back to it.
fn round_trips<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let text = serde_json::to_string(value).unwrap();
    assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), value, "{text}");
    let packed = rmp_serde::to_vec_named(value).unwrap();
    assert_eq!(
        &rmp_serde::from_slice::<T>(&packed).unwrap(),
        value,
        "{text}"
    );
}

#[test]
fn every_id_round_trips_in_both_formats() {
    let v7 = Uuid::parse_str(V7).unwrap();
    round_trips(&EventId::new(v7).unwrap());
    round_trips(&MatterId::new(v7).unwrap());
    round_trips(&ThreadId::new(v7).unwrap());
    round_trips(&TurnId::new(v7).unwrap());
    round_trips(&MessageId::new(v7).unwrap());
    round_trips(&ToolCallId::new(v7).unwrap());
    round_trips(&PermissionRequestId::new(v7).unwrap());
    round_trips(&ElicitationId::new(v7).unwrap());
    round_trips(&CheckpointId::new(v7).unwrap());
    round_trips(&CommandId::new(v7).unwrap());
    round_trips(&FileRefId::new(v7).unwrap());
    round_trips(&FolderRefId::new(v7).unwrap());
    round_trips(&ViewEpoch::new(v7).unwrap());

    let started = DomainEvent::from(TurnStarted {
        turn: TurnId::new(v7).unwrap(),
        checkpoint_before: CheckpointId::new(v7).unwrap(),
    });
    round_trips(&started);
    let outcome = SteerOutcome::Injected {
        turn: TurnId::new(v7).unwrap(),
    };
    round_trips(&outcome);
    for status in [
        ThreadStatus::Idle,
        ThreadStatus::Queued {
            behind: ThreadId::new(v7).unwrap(),
        },
        ThreadStatus::PausedAtLimit { resets_at: None },
    ] {
        round_trips(&status);
    }
}

#[test]
fn a_wrong_type_names_what_a_uuid_is_in_each_format() {
    // JSON reads the hyphenated text; MessagePack reads the 16 bytes.
    let json = serde_json::from_str::<ThreadId>("5")
        .unwrap_err()
        .to_string();
    assert!(json.contains("expected a hyphenated UUID"), "{json}");
    let packed = rmp_serde::from_slice::<ThreadId>(&[0x05])
        .unwrap_err()
        .to_string();
    assert!(packed.contains("expected 16 bytes"), "{packed}");
}

#[test]
fn a_wrong_type_names_a_blob_hash() {
    let error = serde_json::from_str::<BlobHash>("5")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("expected a 64-character lowercase hex hash"),
        "{error}"
    );
}
