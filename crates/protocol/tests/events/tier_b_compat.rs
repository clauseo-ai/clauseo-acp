//! R17 PR-E's wire values as an older reader meets them (the orchestrator's condition 2): a
//! newer cause, stop reason or code decodes to its `Unknown`, and a newer field inside a known
//! variant or struct is skipped. The open enums and the structs take no
//! `deny_unknown_fields`, so a reader of this version meets the next version's values the same
//! way; this one checks the forms PR-E adds, each with a field from a later version beside it.

use clauseo_protocol::{
    ErrorCode, MatterPath, NotProtectedCause, PoolFullWhy, Protection, ProtectionTier,
    ProtectionView, StopReason, Timestamp, TurnEnded, TurnFailed,
};

use crate::fixtures::{decodes_in_both, turn_id, unknown_of};

/// Reads `json` as `T` and checks it is `expected`. JSON only: an id is 16 bytes in
/// `MessagePack`, so these forms are not `MessagePack` ones (the corpus pins those).
fn reads<T: serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(json: &str, expected: &T) {
    assert_eq!(
        &serde_json::from_str::<T>(json).unwrap(),
        expected,
        "{json}"
    );
}

/// A newer cause is `Unknown { kind }`, as PR-E's three are to a reader before it, whether a
/// plain string or a map with a payload.
#[test]
fn a_newer_cause_reads_as_unknown() {
    for (json, kind) in [
        (r#""needs_wsl_rule""#, "needs_wsl_rule"),
        (r#"{"blocked_later":{"reason":"policy"}}"#, "blocked_later"),
    ] {
        decodes_in_both(json, &unknown_of::<NotProtectedCause>(kind));
    }
}

/// PR-E's follow-up adds `probe_failed`, a plain string like `needs_wsl_rule` above. #712's
/// reader, whose variants are these, has no such kind, so it reads it as this one reads
/// `needs_wsl_rule`: `Unknown`, which refuses with `sandbox.blocked`, as `probe_failed` does.
#[test]
fn probe_failed_reads_as_unknown_to_a_reader_before_it() {
    const PR_E: [&str; 10] = [
        "Unsupported",
        "NeedsSetUp",
        "SetUpRefused",
        "SetUpFailed",
        "PoolFull",
        "SwitchedOff",
        "Blocked",
        "OtherAccount",
        "PoolOwned",
        "Unknown",
    ];
    let added: Vec<&str> = NotProtectedCause::VARIANT_NAMES
        .into_iter()
        .filter(|name| !PR_E.contains(name))
        .collect();
    assert_eq!(added, ["ProbeFailed"]);
    assert_eq!(
        serde_json::to_string(&NotProtectedCause::ProbeFailed).unwrap(),
        r#""probe_failed""#
    );
    decodes_in_both(r#""probe_failed""#, &NotProtectedCause::ProbeFailed);
    decodes_in_both(
        r#""needs_wsl_rule""#,
        &unknown_of::<NotProtectedCause>("needs_wsl_rule"),
    );
}

/// A newer field inside a known variant is skipped: `Blocked`, `PoolFull` (whose
/// `needs_reset` a reader before PR-E skips the same way) and `SwitchedOff`.
#[test]
fn a_newer_field_inside_a_known_cause_is_skipped() {
    decodes_in_both(
        r#"{"blocked":{"reason":"policy","since":5,"by":{"kind":"gpo"}}}"#,
        &NotProtectedCause::Blocked {
            reason: "policy".to_owned(),
        },
    );
    decodes_in_both(
        r#"{"pool_full":{"size":8,"open":8,"held":0,"owed":0,"set_aside":0,"needs_reset":2,"spare":1}}"#,
        &NotProtectedCause::PoolFull {
            size: 8,
            open: 8,
            held: 0,
            owed: 0,
            set_aside: 0,
            needs_reset: 2,
            why: None,
        },
    );
    // A host before PR-E writes no `needs_reset`: 0.
    decodes_in_both(
        r#"{"pool_full":{"size":8,"open":8,"held":0,"owed":0,"set_aside":0}}"#,
        &NotProtectedCause::PoolFull {
            size: 8,
            open: 8,
            held: 0,
            owed: 0,
            set_aside: 0,
            needs_reset: 0,
            why: None,
        },
    );
}

/// `PoolFull`'s `why`: each value's wire form, a newer value as `Unknown`, and none written when
/// not known (a reader before it skips the field as any other).
#[test]
fn pool_full_why_reads_known_and_newer_values() {
    let full = |why| NotProtectedCause::PoolFull {
        size: 8,
        open: 8,
        held: 0,
        owed: 0,
        set_aside: 0,
        needs_reset: 1,
        why,
    };
    let head = r#""size":8,"open":8,"held":0,"owed":0,"set_aside":0,"needs_reset":1"#;
    for (why, wire) in [
        (PoolFullWhy::NoFreeSlot, "no_free_slot"),
        (PoolFullWhy::MatterSessionsFull, "matter_sessions_full"),
    ] {
        let json = format!(r#"{{"pool_full":{{{head},"why":"{wire}"}}}}"#);
        assert_eq!(serde_json::to_string(&full(Some(why))).unwrap(), json);
        decodes_in_both(&json, &full(Some(why)));
    }
    decodes_in_both(
        &format!(r#"{{"pool_full":{{{head},"why":"disk_quota"}}}}"#),
        &full(Some(PoolFullWhy::Unknown)),
    );
    assert_eq!(
        serde_json::to_string(&full(None)).unwrap(),
        format!(r#"{{"pool_full":{{{head}}}}}"#)
    );
}

/// A view with `set_up_at` and a later field reads; one without `set_up_at` reads as `None`.
#[test]
fn the_view_skips_a_newer_field_and_reads_without_set_up_at() {
    let mut view = ProtectionView::new(
        Protection::Protected {
            tier: ProtectionTier::B,
        },
        false,
        false,
    );
    view.set_up_at = Some(Timestamp::new(1_793_000_000_000).unwrap());
    decodes_in_both(
        r#"{"tier":"b","computer":{"protected":{"tier":"b"}},"required":false,"unprotected_auto_allowed":false,"set_up_at":1793000000000,"pool":{"size":8}}"#,
        &view,
    );
    view.set_up_at = None;
    decodes_in_both(
        r#"{"tier":"b","computer":{"protected":{"tier":"b"}},"required":false,"unprotected_auto_allowed":false}"#,
        &view,
    );
    assert!(!serde_json::to_string(&view).unwrap().contains("set_up_at"));
}

/// A newer stop reason is `StopReason::Unknown` inside a known `TurnEnded`.
#[test]
fn a_newer_stop_reason_reads_as_unknown() {
    reads(
        &format!(r#"{{"turn":"{}","stop":"left_the_building"}}"#, turn_id(3)),
        &TurnEnded {
            turn: turn_id(3),
            stop: StopReason::Unknown,
            usage: None,
            checkpoint_after: None,
        },
    );
}

/// `TurnFailed` with `at` and a later field reads; a newer code is `Unknown` with the code in
/// front of `detail`, and `at` is kept; without `at` it reads as `None` and writes none.
#[test]
fn turn_failed_keeps_at_skips_a_newer_field_and_moves_a_newer_code() {
    let at = MatterPath::new("Pleadings/Draft.docx").unwrap();
    reads(
        &format!(
            r#"{{"turn":"{}","code":"sandbox.pool_owned","at":"Pleadings/Draft.docx","hint":1}}"#,
            turn_id(3)
        ),
        &TurnFailed {
            turn: turn_id(3),
            code: ErrorCode::SandboxPoolOwned,
            detail: None,
            at: Some(at.clone()),
        },
    );
    reads(
        &format!(
            r#"{{"turn":"{}","code":"sandbox.later","detail":"x","at":"Pleadings/Draft.docx"}}"#,
            turn_id(3)
        ),
        &TurnFailed {
            turn: turn_id(3),
            code: ErrorCode::Unknown,
            detail: Some("sandbox.later: x".to_owned()),
            at: Some(at),
        },
    );
    let none = TurnFailed {
        turn: turn_id(3),
        code: ErrorCode::SandboxPoolFull,
        detail: None,
        at: None,
    };
    reads(
        &format!(r#"{{"turn":"{}","code":"sandbox.pool_full"}}"#, turn_id(3)),
        &none,
    );
    assert!(!serde_json::to_string(&none).unwrap().contains(r#""at""#));
}

/// PR-E's four codes keep their dotted strings and retryable flags.
#[test]
fn the_four_new_codes_have_their_strings_and_retry() {
    for (code, wire, retryable) in [
        (ErrorCode::SandboxPoolFull, "sandbox.pool_full", true),
        (ErrorCode::SandboxPoolOwned, "sandbox.pool_owned", false),
        (ErrorCode::SandboxCantLabel, "sandbox.cant_label", false),
        (ErrorCode::SandboxRemoving, "sandbox.removing", true),
    ] {
        assert_eq!(code.as_str(), wire);
        assert_eq!(code.is_retryable(), retryable, "{wire}");
        decodes_in_both(&format!(r#""{wire}""#), &code);
    }
    decodes_in_both(r#""sandbox.later""#, &ErrorCode::Unknown);
}
