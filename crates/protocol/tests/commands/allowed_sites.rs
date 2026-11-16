//! The lawyer's override of the unsafe-site block list (decision record 0053, PR 2): the two
//! commands' JSON and shape checks, the query and its answer, and the new error code.

use clauseo_protocol::{
    AllowScope, AllowedSite, ErrorCode, HostCommand, HostQuery, MAX_ALLOWED_HOST_BYTES,
    QueryResult, Timestamp,
};

use super::Validate;
use super::fixtures::{blocked_site_id, matter_id, round_trips};

fn revoke(host: &str, scope: AllowScope, matter: bool) -> HostCommand {
    HostCommand::RevokeAllowedSite {
        host: host.to_owned(),
        scope,
        matter: matter.then(|| matter_id(1)),
    }
}

#[test]
fn allow_has_its_json_and_passes_validate() {
    let allow = HostCommand::AllowBlockedSite {
        block_id: blocked_site_id(3),
        scope: AllowScope::Always,
    };
    let json = serde_json::to_string(&allow).unwrap();
    assert!(
        json.starts_with(r#"{"allow_blocked_site":{"block_id":""#),
        "{json}"
    );
    assert!(json.ends_with(r#"","scope":"always"}}"#), "{json}");
    round_trips(&allow);
    assert!(allow.validate().is_ok());
    for bad in [
        r#"{"allow_blocked_site":{"block_id":"0190a5f0-0000-7000-8000-000000000003","scope":"forever"}}"#,
        r#"{"allow_blocked_site":{"block_id":"0190a5f0-0000-7000-8000-000000000003"}}"#,
        r#"{"allow_blocked_site":{"block_id":"0190a5f0-0000-7000-8000-000000000003","scope":"matter","host":"x.test"}}"#,
    ] {
        assert!(serde_json::from_str::<HostCommand>(bad).is_err(), "{bad}");
    }
}

#[test]
fn revoke_names_a_matter_exactly_for_a_matter_allow() {
    let always = revoke("evil.example", AllowScope::Always, false);
    assert_eq!(
        serde_json::to_string(&always).unwrap(),
        r#"{"revoke_allowed_site":{"host":"evil.example","scope":"always"}}"#
    );
    round_trips(&always);
    assert!(always.validate().is_ok());
    assert!(
        revoke("[2606:4700::1111]", AllowScope::Matter, true)
            .validate()
            .is_ok()
    );
    assert!(
        revoke("evil.example", AllowScope::Matter, false)
            .validate()
            .is_err()
    );
    assert!(
        revoke("evil.example", AllowScope::Always, true)
            .validate()
            .is_err()
    );
}

#[test]
fn revoke_refuses_an_empty_spaced_or_long_host() {
    let longest = "a".repeat(MAX_ALLOWED_HOST_BYTES);
    assert!(
        revoke(&longest, AllowScope::Always, false)
            .validate()
            .is_ok()
    );
    let long = "a".repeat(MAX_ALLOWED_HOST_BYTES + 1);
    for host in ["", "evil .example", "evil.example\n", long.as_str()] {
        assert!(
            revoke(host, AllowScope::Always, false).validate().is_err(),
            "{host:?}"
        );
    }
}

#[test]
fn the_query_is_a_plain_string_and_its_answer_names_each_field() {
    assert_eq!(
        serde_json::to_string(&HostQuery::AllowedSites).unwrap(),
        r#""allowed_sites""#
    );
    round_trips(&HostQuery::AllowedSites);
    let at = Timestamp::new(1_794_031_200_000).unwrap();
    let answer = QueryResult::AllowedSites(vec![
        AllowedSite {
            host: "phish.test".to_owned(),
            scope: AllowScope::Always,
            matter: None,
            allowed_at: at,
            saved: true,
        },
        AllowedSite {
            host: "evil.example".to_owned(),
            scope: AllowScope::Matter,
            matter: Some(matter_id(1)),
            allowed_at: at,
            saved: false,
        },
    ]);
    assert_eq!(
        serde_json::to_string(&answer).unwrap(),
        format!(
            r#"{{"allowed_sites":[{{"host":"phish.test","scope":"always","allowed_at":1794031200000,"saved":true}},{{"host":"evil.example","scope":"matter","matter":"{}","allowed_at":1794031200000,"saved":false}}]}}"#,
            matter_id(1)
        )
    );
    round_trips(&answer);
}

#[test]
fn the_override_off_code_is_not_retryable() {
    let code = ErrorCode::BlocklistOverrideOff;
    assert_eq!(code.as_str(), "blocklist.override_off");
    assert!(!code.is_retryable());
    assert_eq!(ErrorCode::from_str_lossy("blocklist.override_off"), code);
}

#[test]
fn the_notice_expired_code_is_not_retryable() {
    let code = ErrorCode::BlocklistNoticeExpired;
    assert_eq!(code.as_str(), "blocklist.notice_expired");
    assert!(!code.is_retryable());
    assert_eq!(ErrorCode::from_str_lossy("blocklist.notice_expired"), code);
}

#[test]
fn the_not_saved_codes_are_not_retryable() {
    for (code, text) in [
        (
            ErrorCode::BlocklistAllowNotSaved,
            "blocklist.allow_not_saved",
        ),
        (
            ErrorCode::BlocklistRevokeNotSaved,
            "blocklist.revoke_not_saved",
        ),
    ] {
        assert_eq!(code.as_str(), text);
        assert!(!code.is_retryable());
        assert_eq!(ErrorCode::from_str_lossy(text), code);
    }
}
