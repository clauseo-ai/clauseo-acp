//! Ids and small values: constructors and `Deserialize` apply the same rules.

use std::str::FromStr;

use uuid::Uuid;

use crate::{
    AgentId, AgentSessionId, BlobHash, CommandId, ContentId, Cursor, MAX_SAFE, MatterPath, Seq,
    ThreadId, Timestamp,
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
    assert!(ThreadId::new(Uuid::parse_str(V4).unwrap()).is_err());
    assert!(ThreadId::new(Uuid::nil()).is_err());
    assert!(
        ThreadId::from_str(&V7.replace('-', "")).is_err(),
        "only the hyphenated form"
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
    assert!(rmp_serde::from_slice::<ThreadId>(&v4).is_err());
}

#[test]
fn command_ids_may_be_any_uuid_but_nil() {
    assert!(CommandId::from_str(V4).is_ok());
    assert!(CommandId::from_str(V7).is_ok());
    assert!(CommandId::from_str(NIL).is_err());
    assert!(json::<CommandId>(&format!("\"{NIL}\"")).is_err());
}

#[test]
fn numbers_stay_below_two_to_the_53() {
    assert!(Seq::new(0).is_err());
    assert_eq!(Seq::new(1).unwrap(), Seq::FIRST);
    assert_eq!(Seq::new(MAX_SAFE).unwrap().get(), MAX_SAFE);
    assert!(Seq::new(MAX_SAFE + 1).is_err());
    assert!(Seq::new(MAX_SAFE).unwrap().next().is_err());
    assert_eq!(Seq::FIRST.next().unwrap().get(), 2);
    assert_eq!(Cursor::new(0).unwrap(), Cursor::ZERO);
    assert!(Cursor::new(MAX_SAFE + 1).is_err());
    assert!(json::<Seq>("0").is_err());
    assert!(json::<Seq>("9007199254740992").is_err());
    assert_eq!(json::<Seq>("9007199254740991").unwrap().get(), MAX_SAFE);
    assert_eq!(serde_json::to_string(&Seq::FIRST).unwrap(), "1");

    let max = i64::try_from(MAX_SAFE).unwrap();
    assert!(Timestamp::new(-max).is_ok());
    assert!(Timestamp::new(max + 1).is_err());
    assert!(Timestamp::new(i64::MIN).is_err());
    assert_eq!(json::<Timestamp>("-5").unwrap().as_millis(), -5);
    assert!(json::<Timestamp>("9007199254740992").is_err());
}

#[test]
fn blob_hash_is_lowercase_hex_in_every_format() {
    let mut bytes = [0u8; 32];
    bytes[0] = 0xab;
    bytes[31] = 0x01;
    let hash = BlobHash::from_bytes(bytes);
    let text = format!("ab{}01", "00".repeat(30));
    assert_eq!(hash.to_string(), text);
    assert_eq!(BlobHash::from_str(&text).unwrap(), hash);
    assert_eq!(serde_json::to_string(&hash).unwrap(), format!("\"{text}\""));
    let packed = rmp_serde::to_vec_named(&hash).unwrap();
    assert_eq!(packed, rmp_serde::to_vec_named(&text).unwrap());
    assert_eq!(rmp_serde::from_slice::<BlobHash>(&packed).unwrap(), hash);
    for bad in [
        text.to_uppercase(),
        text.get(..62).unwrap().to_owned(),
        format!("{text}00"),
        "zz".repeat(32),
    ] {
        assert!(BlobHash::from_str(&bad).is_err(), "{bad}");
    }
    assert!(BlobHash::from_str(&format!("é{}", "0".repeat(62))).is_err());
}

#[test]
fn string_ids_check_their_format() {
    assert!(AgentId::new("claude-acp").is_ok());
    assert!(AgentId::new(&"a".repeat(64)).is_ok());
    for bad in ["", "Claude", "a_b", "a b", &"a".repeat(65)] {
        assert!(AgentId::new(bad).is_err(), "{bad:?}");
    }
    assert!(json::<AgentId>("\"Codex\"").is_err());
    assert_eq!(
        json::<AgentId>("\"codex-acp\"").unwrap().as_str(),
        "codex-acp"
    );

    assert!(ContentId::new("0123abcdef").is_ok());
    assert!(ContentId::new("ABC").is_err());
    assert!(ContentId::new("").is_err());
    assert!(AgentSessionId::new("sess_01").is_ok());
    assert!(AgentSessionId::new("").is_err());
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
    let bad = [
        "", "/a", "a/", "a//b", "./a", "a/./b", "../a", "a/..", "a\\b", "a\0b",
    ];
    for path in bad {
        assert!(MatterPath::new(path).is_err(), "{path:?}");
    }
    assert!(json::<MatterPath>(r#""../etc/passwd""#).is_err());
}

#[test]
fn errors_map_to_command_invalid() {
    let error = ThreadId::from_str(V4).unwrap_err();
    assert_eq!(error.code(), crate::ErrorCode::CommandInvalid);
}

#[test]
fn matter_paths_refuse_drives_controls_and_long_names() {
    for drive in ["C:", "C:x", "c:/Users/a.docx", "z:a/b"] {
        assert!(MatterPath::new(drive).is_err(), "{drive:?}");
    }
    // A `:` elsewhere is a legal macOS file name character.
    for good in ["ab:c", "notes/C:x", "1:/a", "x/a:b"] {
        assert!(MatterPath::new(good).is_ok(), "{good:?}");
    }
    for control in [
        "a\u{1}b", "a\tb", "a\nb", "a\u{1f}", "a\u{7f}b", "a\0b", "a\u{85}b", "a\u{9b}b", "\u{80}",
    ] {
        assert!(MatterPath::new(control).is_err(), "{control:?}");
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
    assert!(MatterPath::new(&format!("{segment}s")).is_err());
    assert!(MatterPath::new(&format!("a/{segment}s/b")).is_err());
    // 4096 bytes: 15 segments of 255 bytes, one of 254, then `/x`, with 16 slashes in all.
    let mut at_limit = vec![segment.as_str(); 15].join("/");
    at_limit.push('/');
    at_limit.push_str(&"s".repeat(254));
    at_limit.push_str("/x");
    assert_eq!(at_limit.len(), 4096);
    assert!(MatterPath::new(&at_limit).is_ok());
    at_limit.push('x');
    assert!(MatterPath::new(&at_limit).is_err());
    assert!(json::<MatterPath>(r#""C:/x""#).is_err());
    assert!(json::<MatterPath>(r#""a\u0001""#).is_err());
}

#[test]
fn uuid_ids_decode_only_the_hyphenated_text_or_16_bytes() {
    let simple = V7.replace('-', "");
    let braced = format!("{{{V7}}}");
    let urn = format!("urn:uuid:{V7}");
    for form in [simple, braced, urn] {
        assert!(json::<ThreadId>(&format!("\"{form}\"")).is_err(), "{form}");
    }
    assert!(json::<ThreadId>(&format!("\"{V7}\"")).is_ok());
    // MessagePack: a str, or bin of another length, is refused.
    let as_str = rmp_serde::to_vec_named(V7).unwrap();
    assert!(rmp_serde::from_slice::<ThreadId>(&as_str).is_err());
    let id = ThreadId::from_str(V7).unwrap();
    let mut short =
        rmp_serde::to_vec_named(&serde_bytes_of(&id.as_uuid().as_bytes()[..15])).unwrap();
    assert!(rmp_serde::from_slice::<ThreadId>(&short).is_err());
    short = rmp_serde::to_vec_named(&serde_bytes_of(&[7u8; 17])).unwrap();
    assert!(rmp_serde::from_slice::<ThreadId>(&short).is_err());
    let packed = rmp_serde::to_vec_named(&id).unwrap();
    assert_eq!(rmp_serde::from_slice::<ThreadId>(&packed).unwrap(), id);
    // CommandId keeps its own rule: any version, never nil, strict forms alike.
    assert!(json::<CommandId>(&format!("\"{V4}\"")).is_ok());
    assert!(json::<CommandId>(&format!("\"{}\"", V4.replace('-', ""))).is_err());
    let nil = rmp_serde::to_vec_named(&Uuid::nil()).unwrap();
    assert!(rmp_serde::from_slice::<CommandId>(&nil).is_err());
}

/// Bytes that serialise as `MessagePack` bin.
fn serde_bytes_of(bytes: &[u8]) -> crate::RawPayload {
    crate::RawPayload::Bytes(bytes.into())
}
