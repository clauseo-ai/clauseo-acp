//! The sandbox's wire (decision record 0051): `HostQuery::Protection` and its answer, the two
//! commands, `MatterSummary::protection`, and `NotProtectedCause` decoding a newer cause.

use clauseo_protocol::{
    CommandEnvelope, HostCommand, HostQuery, MatterSummary, NotProtectedCause, Protection,
    ProtectionTier, ProtectionView, QueryResult, Timestamp,
};

use super::Validate;
use super::fixtures::{command_id, decodes_in_both, round_trips, unknown_of};

#[test]
fn the_query_is_a_plain_string_and_its_answer_names_each_field() {
    assert_eq!(
        serde_json::to_string(&HostQuery::Protection).unwrap(),
        r#""protection""#
    );
    round_trips(&HostQuery::Protection);
    let answer = QueryResult::Protection(ProtectionView {
        tier: ProtectionTier::C,
        computer: Protection::NotProtected {
            cause: NotProtectedCause::NeedsSetUp,
        },
        required: false,
        unprotected_auto_allowed: true,
    });
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        r#"{"protection":{"tier":"c","computer":{"not_protected":{"cause":"needs_set_up"}},"required":false,"unprotected_auto_allowed":true}}"#
    );
    round_trips(&answer);
}

#[test]
fn every_tier_and_cause_has_its_wire_form() {
    for (tier, json) in ProtectionTier::ALL.into_iter().zip(["a", "b", "c"]) {
        let protected = Protection::Protected { tier };
        assert_eq!(
            serde_json::to_string(&protected).unwrap(),
            format!(r#"{{"protected":{{"tier":"{json}"}}}}"#)
        );
        round_trips(&protected);
    }
    let at = Timestamp::new(1_760_000_000_000).unwrap();
    for (cause, json) in [
        (NotProtectedCause::Unsupported, r#""unsupported""#),
        (NotProtectedCause::NeedsSetUp, r#""needs_set_up""#),
        (NotProtectedCause::SetUpRefused, r#""set_up_refused""#),
        (NotProtectedCause::SetUpFailed, r#""set_up_failed""#),
        (
            NotProtectedCause::PoolFull {
                size: 8,
                open: 3,
                held: 2,
                owed: 2,
                set_aside: 1,
            },
            r#"{"pool_full":{"size":8,"open":3,"held":2,"owed":2,"set_aside":1}}"#,
        ),
        (
            NotProtectedCause::SwitchedOff { at },
            r#"{"switched_off":{"at":1760000000000}}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&cause).unwrap(), json);
        decodes_in_both(json, &cause);
        round_trips(&cause);
    }
}

/// A cause from a newer host decodes to `Unknown` and keeps the rest of the answer; a known
/// cause in the wrong form, or a tier this version does not know, still fails.
#[test]
fn a_newer_cause_reads_as_unknown_and_a_newer_tier_fails() {
    let newer = r#"{"not_protected":{"cause":{"jail_paused":{"until":5}}}}"#;
    let expected = Protection::NotProtected {
        cause: unknown_of::<NotProtectedCause>("jail_paused"),
    };
    decodes_in_both(newer, &expected);
    assert_eq!(
        serde_json::to_string(&expected).unwrap(),
        r#"{"not_protected":{"cause":"jail_paused"}}"#
    );
    for bad in [
        r#"{"not_protected":{"cause":"pool_full"}}"#,
        r#"{"not_protected":{"cause":{"unsupported":{}}}}"#,
        r#"{"protected":{"tier":"d"}}"#,
        r#""protected""#,
    ] {
        assert!(serde_json::from_str::<Protection>(bad).is_err(), "{bad}");
    }
}

/// An older host's row has no `protection`, which reads as not known; a row without one writes
/// none.
#[test]
fn a_row_without_protection_reads_as_none_and_writes_none() {
    let row: MatterSummary = serde_json::from_str(
        r#"{"matter":"0190a5f0-0000-7000-8000-000000000001","name":"Smith","kind":"folder"}"#,
    )
    .unwrap();
    assert_eq!(row.protection, None);
    assert!(!serde_json::to_string(&row).unwrap().contains("protection"));
    let protected = MatterSummary {
        protection: Some(Protection::Protected {
            tier: ProtectionTier::A,
        }),
        ..row
    };
    assert!(
        serde_json::to_string(&protected)
            .unwrap()
            .contains(r#""protection":{"protected":{"tier":"a"}}"#)
    );
    round_trips(&protected);
}

#[test]
fn the_commands_have_their_json_pass_validate_and_refuse_fields() {
    for (command, json) in [
        (
            HostCommand::RetryProtection {},
            r#"{"retry_protection":{}}"#,
        ),
        (
            HostCommand::RemoveSandboxSetup {},
            r#"{"remove_sandbox_setup":{}}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&command).unwrap(), json);
        round_trips(&command);
        assert!(command.validate().is_ok(), "{json} refused");
    }
    for body in [
        r#"{"retry_protection":{"matter":"0190a5f0-0000-7000-8000-000000000001"}}"#,
        r#"{"remove_sandbox_setup":{"force":true}}"#,
        r#""retry_protection""#,
    ] {
        let json = format!(r#"{{"command_id":"{}","command":{body}}}"#, command_id(1));
        assert!(
            serde_json::from_str::<CommandEnvelope>(&json).is_err(),
            "{body} decoded"
        );
    }
}
