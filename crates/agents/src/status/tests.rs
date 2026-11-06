//! The status parsers, table-driven: Claude's JSON and its detection and sign-in verdicts,
//! Codex's exit codes and `account/read`, and the sign-in-again text.

use clauseo_codex_appserver::GetAccountResponse;

use super::{
    ClaudeStatus, CodexDetect, Identity, claude_detection, claude_signed_in, codex_identity,
    codex_logged_in, is_sign_in_again, parse_claude_status,
};
use crate::signin::{Found, LoginFrom, SigninState};

/// The fake's signed-in line (the brief, "The fakes").
const SIGNED_IN: &str = r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"test@example.com","subscriptionType":"max"}"#;

fn status(auth_method: &str, api_provider: Option<&str>) -> ClaudeStatus {
    ClaudeStatus {
        logged_in: auth_method != "none",
        auth_method: Some(auth_method.to_owned()),
        api_provider: api_provider.map(str::to_owned),
        email: Some("lawyer@firm.example".to_owned()),
        subscription_type: Some("pro".to_owned()),
    }
}

#[test]
fn claude_status_parses_with_unknown_fields_ignored() {
    let table: [(&str, Option<ClaudeStatus>); 5] = [
        (
            SIGNED_IN,
            Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("claude.ai".to_owned()),
                api_provider: Some("firstParty".to_owned()),
                email: Some("test@example.com".to_owned()),
                subscription_type: Some("max".to_owned()),
            }),
        ),
        (
            r#"{"loggedIn":false,"authMethod":"none","apiKeySource":"none","configDirectory":"/x"}"#,
            Some(ClaudeStatus {
                logged_in: false,
                auth_method: Some("none".to_owned()),
                ..ClaudeStatus::default()
            }),
        ),
        (
            "A notice first\n{\"loggedIn\":true,\"authMethod\":\"api_key\",\"orgId\":7}\n",
            Some(ClaudeStatus {
                logged_in: true,
                auth_method: Some("api_key".to_owned()),
                ..ClaudeStatus::default()
            }),
        ),
        ("{}", Some(ClaudeStatus::default())),
        ("not json", None),
    ];
    for (text, want) in table {
        assert_eq!(parse_claude_status(text), want, "{text}");
    }
}

#[test]
fn claude_detection_finds_only_a_first_party_claude_ai_login() {
    let found = |email: &str| SigninState::LoginFound {
        from: LoginFrom::ClaudeCode,
        email: Some(email.to_owned()),
    };
    let out = |found| SigninState::SignedOut { found };
    let table = [
        (
            Some(0),
            Some(status("claude.ai", Some("firstParty"))),
            found("lawyer@firm.example"),
        ),
        (
            Some(0),
            Some(status("claude.ai", None)),
            found("lawyer@firm.example"),
        ),
        (
            Some(0),
            Some(status("claude.ai", Some("bedrock"))),
            out(Some(Found::ClaudeSettings)),
        ),
        (
            Some(0),
            Some(status("third_party", Some("vertex"))),
            out(Some(Found::ClaudeSettings)),
        ),
        (
            Some(0),
            Some(status("api_key_helper", None)),
            out(Some(Found::ClaudeSettings)),
        ),
        (
            Some(0),
            Some(status("api_key", None)),
            out(Some(Found::ClaudeSettings)),
        ),
        (
            Some(0),
            Some(status("oauth_token", None)),
            out(Some(Found::ClaudeSettings)),
        ),
        (
            Some(0),
            Some(status("none", None)),
            out(Some(Found::NoLogin)),
        ),
        (
            Some(1),
            Some(status("claude.ai", Some("firstParty"))),
            out(Some(Found::NoLogin)),
        ),
        (
            None,
            Some(status("claude.ai", Some("firstParty"))),
            out(Some(Found::NoLogin)),
        ),
        (Some(0), None, out(Some(Found::NoLogin))),
    ];
    for (exit, status, want) in table {
        assert_eq!(
            claude_detection(exit, status.as_ref()),
            want,
            "{exit:?} {status:?}"
        );
    }
    let parsed = parse_claude_status(SIGNED_IN);
    assert_eq!(
        claude_detection(Some(0), parsed.as_ref()),
        found("test@example.com")
    );
}

#[test]
fn claude_signed_in_reads_the_account_and_plan() {
    let identity = Identity {
        email: Some("lawyer@firm.example".to_owned()),
        plan: Some("pro".to_owned()),
    };
    let table = [
        (
            Some(0),
            Some(status("claude.ai", Some("firstParty"))),
            Some(identity.clone()),
        ),
        (Some(0), Some(status("api_key", None)), Some(identity)),
        (Some(0), Some(status("none", None)), None),
        (Some(1), Some(status("claude.ai", None)), None),
        (Some(0), None, None),
    ];
    for (exit, status, want) in table {
        assert_eq!(
            claude_signed_in(exit, status.as_ref()),
            want,
            "{exit:?} {status:?}"
        );
    }
}

#[test]
fn codex_detection_reads_the_exit_code_and_the_stat() {
    assert!(codex_logged_in(Some(0)));
    for exit in [Some(1), Some(2), None] {
        assert!(!codex_logged_in(exit));
    }
    let email = || Some("test@example.com".to_owned());
    let table = [
        (Some(0), true, CodexDetect::Reusable),
        (Some(0), false, CodexDetect::Keyring),
        (Some(1), true, CodexDetect::SignedOut),
        (None, false, CodexDetect::SignedOut),
    ];
    for (exit, present, want) in table {
        assert_eq!(CodexDetect::classify(exit, present), want);
    }
    assert_eq!(
        CodexDetect::Reusable.state(email()),
        SigninState::LoginFound {
            from: LoginFrom::Codex,
            email: email()
        }
    );
    assert_eq!(
        CodexDetect::Keyring.state(email()),
        SigninState::SignedOut {
            found: Some(Found::CodexKeyring)
        }
    );
    assert_eq!(
        CodexDetect::SignedOut.state(None),
        SigninState::SignedOut {
            found: Some(Found::NoLogin)
        }
    );
}

#[test]
fn account_read_gives_a_chatgpt_account_alone() {
    let identity = |email: Option<&str>, plan: &str| {
        Some(Identity {
            email: email.map(str::to_owned),
            plan: Some(plan.to_owned()),
        })
    };
    let table = [
        (
            r#"{"account":{"type":"chatgpt","email":"test@example.com","planType":"plus"},"requiresOpenaiAuth":true}"#,
            identity(Some("test@example.com"), "plus"),
        ),
        (
            r#"{"account":{"type":"chatgpt","email":null,"planType":"newplan"},"requiresOpenaiAuth":true}"#,
            identity(None, "newplan"),
        ),
        (
            r#"{"account":{"type":"apiKey"},"requiresOpenaiAuth":true}"#,
            None,
        ),
        (
            r#"{"account":{"type":"amazonBedrock"},"requiresOpenaiAuth":false}"#,
            None,
        ),
        (
            r#"{"account":{"type":"later"},"requiresOpenaiAuth":true}"#,
            None,
        ),
        (r#"{"requiresOpenaiAuth":true}"#, None),
    ];
    for (json, want) in table {
        let response: GetAccountResponse = serde_json::from_str(json).unwrap();
        assert_eq!(codex_identity(&response), want, "{json}");
    }
}

#[test]
fn sign_in_again_is_a_refresh_token_failure() {
    let table = [
        ("refresh_token_reused", true),
        (
            "Your access token could not be refreshed (code: REFRESH_TOKEN_REUSED). Please log out and sign in again.",
            true,
        ),
        ("error: refresh_token_expired", true),
        ("{\"code\":\"refresh_token_invalidated\"}", true),
        ("rate limit reached", false),
        ("refresh token", false),
        ("", false),
    ];
    for (text, want) in table {
        assert_eq!(is_sign_in_again(text), want, "{text}");
    }
}
