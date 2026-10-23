//! The unit-only enums opened when 0002 made every enum in a logged type open, and decoding
//! rules of the open enums seen through whole events. Each open enum's own round trip (every
//! value of `ALL`, or one sample per row) is a test its macro writes beside it.

use clauseo_protocol::{
    CompactionStatus, ConfigCategory, ContentBlock, Delivery, DomainEvent, ElicitationOutcome,
    Envelope, LimitStopReason, MatterKind, PermissionChoice, PlanPriority, PlanStatus, Principal,
    RawFrom, RawPayload, RestoreScope, TextKind, TextOrBlob, ToolKind,
};

use crate::examples::every_event;
use crate::fixtures::{decode_both, decodes_in_both, entry, envelope, packed_raw};
use crate::json_copies::ID_KEY;
use crate::unknown_variants::unit_enum;

#[test]
fn principal_keeps_its_wire_form() {
    let json = r#""local_desktop""#;
    assert_eq!(
        serde_json::to_string(&Principal::LocalDesktop).unwrap(),
        json
    );
    assert_eq!(
        serde_json::from_str::<Principal>(json).unwrap(),
        Principal::LocalDesktop
    );
}

/// Checks that `json` fails to decode as `T` in both formats with an error that says `says`.
fn refused_saying<T: serde::de::DeserializeOwned + std::fmt::Debug>(json: &str, says: &str) {
    let (from_json, from_packed) = decode_both::<T>(json);
    for decoded in [from_json, from_packed] {
        let error = decoded.unwrap_err();
        assert!(error.contains(says), "{json}: {error}");
    }
}

#[test]
fn a_unit_variant_as_a_key_is_refused_whatever_its_payload() {
    let cases = [
        r#"{"local_desktop":{"device":"x"}}"#,
        r#"{"local_desktop":{}}"#,
        r#"{"local_desktop":null}"#,
        r#"{"local_desktop":5}"#,
    ];
    let says = "`local_desktop` has no fields, so it is a plain string, not a map";
    for json in cases {
        refused_saying::<Principal>(json, says);
    }
    let says = "`cancelled` has no fields";
    refused_saying::<PermissionChoice>(r#"{"cancelled":{"by_turn":true}}"#, says);
    refused_saying::<PermissionChoice>(r#"{"cancelled":[1,2]}"#, says);
    refused_saying::<ToolKind>(r#"{"read":{"scope":"x"}}"#, "`read` has no fields");
    refused_saying::<Delivery>(r#"{"queue":{"after":3}}"#, "`queue` has no fields");
    refused_saying::<PlanPriority>(r#"{"high":null}"#, "`high` has no fields");
    refused_saying::<PlanPriority>(r#"{"high":5}"#, "`high` has no fields");
    refused_saying::<RestoreScope>(r#"{"matter":"x"}"#, "`matter` has no fields");
}

#[test]
fn an_unknown_kind_takes_null_as_its_payload() {
    decodes_in_both(r#"{"zz":null}"#, &PlanPriority::Unknown);
}

#[test]
fn a_plain_string_naming_a_struct_kind_says_so() {
    let says = "`files` has fields, so it is a one-key map, not a plain string";
    refused_saying::<RestoreScope>(r#""files""#, says);
}

#[test]
fn text_kind() {
    let cases = [(TextKind::Answer, "answer"), (TextKind::Thought, "thought")];
    unit_enum(&TextKind::ALL, &cases, TextKind::Unknown);
}

#[test]
fn raw_from() {
    let cases = [(RawFrom::Acp, "acp"), (RawFrom::Clauseo, "clauseo")];
    unit_enum(&RawFrom::ALL, &cases, RawFrom::Unknown);
}

#[test]
fn elicitation_outcome() {
    use ElicitationOutcome::{Accepted, Cancelled, Declined};
    let cases = [
        (Accepted, "accepted"),
        (Declined, "declined"),
        (Cancelled, "cancelled"),
    ];
    unit_enum(
        &ElicitationOutcome::ALL,
        &cases,
        ElicitationOutcome::Unknown,
    );
}

#[test]
fn plan_priority() {
    use PlanPriority::{High, Low, Medium};
    let cases = [(High, "high"), (Medium, "medium"), (Low, "low")];
    unit_enum(&PlanPriority::ALL, &cases, PlanPriority::Unknown);
}

#[test]
fn plan_status() {
    use PlanStatus::{Completed, InProgress, Pending};
    let cases = [
        (Pending, "pending"),
        (InProgress, "in_progress"),
        (Completed, "completed"),
    ];
    unit_enum(&PlanStatus::ALL, &cases, PlanStatus::Unknown);
}

#[test]
fn config_category() {
    use ConfigCategory::{Mode, Model, Other, ThoughtLevel};
    let cases = [
        (Mode, "mode"),
        (Model, "model"),
        (ThoughtLevel, "thought_level"),
        (Other, "other"),
    ];
    unit_enum(&ConfigCategory::ALL, &cases, ConfigCategory::Unknown);
}

#[test]
fn compaction_status() {
    let cases = [
        (CompactionStatus::Started, "started"),
        (CompactionStatus::Completed, "completed"),
    ];
    unit_enum(&CompactionStatus::ALL, &cases, CompactionStatus::Unknown);
}

#[test]
fn limit_stop_reason() {
    use LimitStopReason::{PlanWindow, Temporary, WorkspaceCredits, WorkspaceSpendCap};
    let cases = [
        (PlanWindow, "plan_window"),
        (WorkspaceCredits, "workspace_credits"),
        (WorkspaceSpendCap, "workspace_spend_cap"),
        (Temporary, "temporary"),
    ];
    unit_enum(&LimitStopReason::ALL, &cases, LimitStopReason::Unknown);
}

#[test]
fn matter_kind() {
    let cases = [
        (MatterKind::Scratch, "scratch"),
        (MatterKind::Folder, "folder"),
    ];
    unit_enum(&MatterKind::ALL, &cases, MatterKind::Unknown);
}

#[test]
fn a_block_with_a_binary_key_keeps_its_json_copy() {
    // A newer block read from MessagePack, holding a map keyed by an id (16 bytes): its JSON copy
    // writes the key as the text of its list of byte numbers, and the envelope still decodes.
    let appended = every_event()
        .into_iter()
        .find(|event| event.kind() == "content_appended")
        .unwrap();
    let mut packed = packed_raw(&envelope(appended));
    let payload = entry(entry(&mut packed, "event"), "content_appended");
    let by_id =
        RawPayload::Map(vec![(RawPayload::Bytes(vec![7; 16].into()), RawPayload::U64(1))].into());
    *entry(payload, "block") =
        RawPayload::Map(vec![(RawPayload::Str("hologram".into()), by_id)].into());
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let read: Envelope = rmp_serde::from_slice(&bytes).unwrap();
    let DomainEvent::ContentAppended(appended) = read.event else {
        panic!("not content_appended: {:?}", read.event);
    };
    let expected = ContentBlock::Unsupported {
        kind: "hologram".to_owned(),
        raw_from: RawFrom::Clauseo,
        raw: Some(TextOrBlob::Inline(format!(
            r#"{{"hologram":{{"{ID_KEY}":1}}}}"#
        ))),
    };
    assert_eq!(appended.block, expected);
}

#[test]
fn decode_errors_name_the_public_enum() {
    let error = serde_json::from_str::<ContentBlock>(r#"{"text":"x"}"#).unwrap_err();
    assert!(error.to_string().contains("ContentBlock::Text"), "{error}");
    let packed = rmp_serde::to_vec_named(&serde_json::json!({"text": null})).unwrap();
    let error = rmp_serde::from_slice::<ContentBlock>(&packed).unwrap_err();
    assert!(error.to_string().contains("ContentBlock::Text"), "{error}");
}

#[test]
fn a_logged_restore_scope_accepts_fields_it_does_not_know() {
    // A newer Clauseo adds a field to `Files`; this version still replays the event.
    let restored = every_event()
        .into_iter()
        .find(|event| event.kind() == "version_restored")
        .unwrap();
    let mut packed = packed_raw(&envelope(restored.clone()));
    let payload = entry(entry(&mut packed, "event"), "version_restored");
    let files = entry(entry(payload, "scope"), "files");
    let RawPayload::Map(fields) = files else {
        panic!("not a map: {files:?}");
    };
    let mut with_extra = fields.to_vec();
    with_extra.push((RawPayload::Str("later".into()), RawPayload::U64(1)));
    *fields = with_extra.into();
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let read: Envelope = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(read.event, restored);
    let json = serde_json::to_string(&envelope(restored.clone())).unwrap();
    let extra = json.replace(r#""a.docx"]}"#, r#""a.docx"],"later":1}"#);
    assert_ne!(extra, json);
    let read: Envelope = serde_json::from_str(&extra).unwrap();
    assert_eq!(read.event, restored);
}
