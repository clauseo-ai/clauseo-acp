//! The setup commands, the secret, the events and the codes; see `../setup.rs`.

use clauseo_protocol::{
    AgentId, Count, ErrorCode, FoundLogin, HostError, InstallLog, InstallProgress, LoginFrom,
    MAX_INSTALL_LOG_LINE_BYTES, MAX_INSTALL_LOG_LINES, MAX_SECRET_BYTES, PlanLabel, Secret,
    SetupAction, SetupCommand, SetupEvent, SignIn, SignInMethod, SignInSource, Timestamp,
};
use zeroize::Zeroizing;

use crate::fixtures::{decode_both, decodes_in_both, round_trips};

/// The secret text the tests send; no output may contain it.
const SECRET: &str = "sk-ant-api03-not-a-real-key";

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn secret(text: &str) -> Secret {
    Secret::new(Zeroizing::new(text.to_owned())).unwrap()
}

fn at(millis: i64) -> Timestamp {
    Timestamp::new(millis).unwrap()
}

/// Each command's JSON and what it decodes to, one per variant, in declaration order.
fn every_command() -> Vec<(String, SetupCommand)> {
    let a = || claude();
    let plain = |name: &str| format!(r#"{{"{name}":{{"agent":"claude-acp"}}}}"#);
    vec![
        (
            plain("install_agent"),
            SetupCommand::InstallAgent { agent: a() },
        ),
        (
            r#"{"retry_agent_install":{"agent":"claude-acp","use_system_proxy":true}}"#.to_owned(),
            SetupCommand::RetryAgentInstall {
                agent: a(),
                use_system_proxy: true,
            },
        ),
        (
            r#"{"start_sign_in":{"agent":"claude-acp","console":true}}"#.to_owned(),
            SetupCommand::StartSignIn {
                agent: a(),
                console: true,
            },
        ),
        (
            plain("cancel_sign_in"),
            SetupCommand::CancelSignIn { agent: a() },
        ),
        (
            format!(r#"{{"paste_sign_in_code":{{"agent":"claude-acp","code":"{SECRET}"}}}}"#),
            SetupCommand::PasteSignInCode {
                agent: a(),
                code: secret(SECRET),
            },
        ),
        (
            plain("use_device_code"),
            SetupCommand::UseDeviceCode { agent: a() },
        ),
        (
            plain("open_sign_in_page"),
            SetupCommand::OpenSignInPage { agent: a() },
        ),
        (plain("sign_out"), SetupCommand::SignOut { agent: a() }),
        (
            plain("use_found_login"),
            SetupCommand::UseFoundLogin { agent: a() },
        ),
        (
            plain("use_different_account"),
            SetupCommand::UseDifferentAccount { agent: a() },
        ),
        (
            plain("stop_using_login"),
            SetupCommand::StopUsingLogin { agent: a() },
        ),
        (plain("find_login"), SetupCommand::FindLogin { agent: a() }),
        (
            format!(r#"{{"set_agent_key":{{"agent":"claude-acp","key":"{SECRET}"}}}}"#),
            SetupCommand::SetAgentKey {
                agent: a(),
                key: secret(SECRET),
            },
        ),
        (
            plain("clear_agent_key"),
            SetupCommand::ClearAgentKey { agent: a() },
        ),
        (
            r#"{"set_firm_endpoint":{"agent":"claude-acp","url":"https://gw.example"}}"#.to_owned(),
            SetupCommand::SetFirmEndpoint {
                agent: a(),
                url: Some("https://gw.example".to_owned()),
            },
        ),
    ]
}

#[test]
fn every_command_decodes_from_its_json() {
    let commands = every_command();
    let actions: Vec<SetupAction> = commands.iter().map(|(_, c)| c.action()).collect();
    assert_eq!(
        actions,
        SetupAction::ALL,
        "one example per command, in order"
    );
    for (json, expected) in &commands {
        decodes_in_both(json, expected);
        assert_eq!(expected.agent(), &claude());
    }
}

#[test]
fn missing_flags_and_url_take_their_defaults() {
    decodes_in_both(
        r#"{"retry_agent_install":{"agent":"claude-acp"}}"#,
        &SetupCommand::RetryAgentInstall {
            agent: claude(),
            use_system_proxy: false,
        },
    );
    decodes_in_both(
        r#"{"start_sign_in":{"agent":"claude-acp"}}"#,
        &SetupCommand::StartSignIn {
            agent: claude(),
            console: false,
        },
    );
    for json in [
        r#"{"set_firm_endpoint":{"agent":"claude-acp"}}"#,
        r#"{"set_firm_endpoint":{"agent":"claude-acp","url":null}}"#,
    ] {
        decodes_in_both(
            json,
            &SetupCommand::SetFirmEndpoint {
                agent: claude(),
                url: None,
            },
        );
    }
}

#[test]
fn a_command_refuses_unknown_fields_commands_and_bad_agents() {
    for json in [
        r#"{"sign_out":{"agent":"claude-acp","extra":1}}"#,
        r#"{"sign_in_harder":{"agent":"claude-acp"}}"#,
        r#"{"sign_out":{"agent":"Claude ACP"}}"#,
        r#"{"sign_out":{}}"#,
        r#""sign_out""#,
    ] {
        let (from_json, from_packed) = decode_both::<SetupCommand>(json);
        assert!(from_json.is_err() && from_packed.is_err(), "{json}");
    }
}

#[test]
fn a_secret_refuses_empty_and_overlong_text_without_quoting_it() {
    // A recognisable start, so an error quoting any part of the text shows.
    let long = format!("sk-{}", "k".repeat(MAX_SECRET_BYTES - 2));
    assert_eq!(long.len(), MAX_SECRET_BYTES + 1);
    for (text, quoted) in [("", "\"\""), (long.as_str(), "sk-k")] {
        let json = format!(r#"{{"set_agent_key":{{"agent":"claude-acp","key":"{text}"}}}}"#);
        let from_json = serde_json::from_str::<SetupCommand>(&json).unwrap_err();
        let shown = [from_json.to_string(), format!("{from_json:?}")];
        let packed = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        let packed = rmp_serde::to_vec_named(&packed).unwrap();
        let from_packed = rmp_serde::from_slice::<SetupCommand>(&packed).unwrap_err();
        let shown_packed = [from_packed.to_string(), format!("{from_packed:?}")];
        for error in shown.iter().chain(&shown_packed) {
            assert!(
                error.contains("a secret must be 1 to 4096 bytes"),
                "{error}"
            );
            assert!(!error.contains(quoted), "the error quotes the key: {error}");
        }
    }
    let full = "k".repeat(MAX_SECRET_BYTES);
    assert_eq!(secret(&full).expose(), full);
    assert!(Secret::new(Zeroizing::new(String::new())).is_none());
    assert!(Secret::new(Zeroizing::new(long)).is_none());
}

#[test]
fn no_command_with_a_secret_shows_it() {
    let mut seen = 0;
    for (_, command) in every_command() {
        let shown = format!("{command:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        if matches!(
            command,
            SetupCommand::PasteSignInCode { .. } | SetupCommand::SetAgentKey { .. }
        ) {
            assert!(shown.contains("<secret>"), "{shown}");
            seen += 1;
        }
    }
    assert_eq!(seen, 2, "every command holding a secret is checked");
    let key = secret(SECRET);
    assert_eq!(format!("{key:?}"), "<secret>");
    assert_eq!(format!("{key}"), "<secret>");
    assert_eq!(format!("{key:#?}"), "<secret>");
    assert_eq!(key.expose(), SECRET);
    assert_eq!(*key.into_zeroizing(), SECRET);
}

/// One example of every sign-in state.
fn every_sign_in() -> Vec<SignIn> {
    let mut states = vec![
        SignIn::SignedOut { found: None },
        SignIn::Queued,
        SignIn::Waiting {
            since: at(1_700_000_000_000),
            nudge: true,
        },
        SignIn::DeviceCode {
            url: "https://auth.openai.com/codex/device".to_owned(),
            code: "ABCD-1234".to_owned(),
            expires_at: at(1_700_000_900_000),
        },
        SignIn::LoginFound {
            from: LoginFrom::ClaudeCode,
            email: None,
        },
        SignIn::LoginFound {
            from: LoginFrom::Codex,
            email: Some("a@firm.example".to_owned()),
        },
        SignIn::Failed {
            code: ErrorCode::SigninPortBusy,
            at: at(5),
        },
        SignIn::Failed {
            code: ErrorCode::AgentAuthRequired,
            at: at(6),
        },
        SignIn::Switching {
            since: at(1_700_000_000_000),
            stuck: false,
        },
        SignIn::Switching {
            since: at(9),
            stuck: true,
        },
    ];
    states.extend(FoundLogin::ALL.map(|found| SignIn::SignedOut { found: Some(found) }));
    for method in SignInMethod::ALL {
        for source in SignInSource::ALL {
            states.push(SignIn::SignedIn {
                email: Some("a@firm.example".to_owned()),
                plan: Some(PlanLabel("Claude Max".to_owned())),
                method,
                source,
                at: at(7),
            });
        }
    }
    states.push(SignIn::SignedIn {
        email: None,
        plan: None,
        method: SignInMethod::ApiKey,
        source: SignInSource::Own,
        at: at(8),
    });
    states
}

/// One example of every install state.
fn every_install() -> Vec<InstallProgress> {
    let log = InstallLog::new([
        "Clauseo download log".to_owned(),
        "agent: claude-acp".to_owned(),
    ]);
    let hosts = vec!["registry.npmjs.org".to_owned(), "nodejs.org".to_owned()];
    vec![
        InstallProgress::Missing {
            download_bytes: Count::from(61_000_000_u32),
        },
        InstallProgress::Waiting {
            code: ErrorCode::AgentInstallOffline,
            hosts: hosts.clone(),
            log: log.clone(),
        },
        InstallProgress::Downloading {
            received: Count::from(1_u32),
            total: Count::from(2_u32),
        },
        InstallProgress::Installing,
        InstallProgress::Ready {
            version: "0.10.2".to_owned(),
        },
        InstallProgress::Failed {
            code: ErrorCode::AgentInstallDiskFull,
            need_mb: Some(Count::from(300_u32)),
            free_mb: Some(Count::from(12_u32)),
            hosts: hosts.clone(),
            log: log.clone(),
        },
        InstallProgress::Failed {
            code: ErrorCode::AgentInstallBlocked,
            need_mb: None,
            free_mb: None,
            hosts,
            log,
        },
        InstallProgress::Paused,
        InstallProgress::Managed,
    ]
}

#[test]
fn every_setup_event_round_trips() {
    round_trips(&SetupEvent::SetupInfo {
        keys_available: false,
    });
    for state in every_install() {
        round_trips(&SetupEvent::AgentInstallProgress {
            agent: claude(),
            state,
        });
    }
    for state in every_sign_in() {
        round_trips(&SetupEvent::SignInChanged {
            agent: claude(),
            state,
        });
    }
}

#[test]
fn setup_events_have_their_json_form() {
    let cases = [
        (
            SetupEvent::SetupInfo {
                keys_available: true,
            },
            r#"{"setup_info":{"keys_available":true}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::Queued,
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":"queued"}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::SignedOut {
                    found: Some(FoundLogin::CodexLinkSplit),
                },
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":{"signed_out":{"found":"codex_link_split"}}}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::SignedOut {
                    found: Some(FoundLogin::NoneFound),
                },
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":{"signed_out":{"found":"none_found"}}}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::SignedOut {
                    found: Some(FoundLogin::ReuseOff),
                },
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":{"signed_out":{"found":"reuse_off"}}}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::Failed {
                    code: ErrorCode::SigninLinkExpired,
                    at: at(9),
                },
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":{"failed":{"code":"signin.link_expired","at":9}}}}"#,
        ),
        (
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::Switching {
                    since: at(9),
                    stuck: true,
                },
            },
            r#"{"sign_in_changed":{"agent":"claude-acp","state":{"switching":{"since":9,"stuck":true}}}}"#,
        ),
        (
            SetupEvent::AgentInstallProgress {
                agent: claude(),
                state: InstallProgress::Missing {
                    download_bytes: Count::from(5_u32),
                },
            },
            r#"{"agent_install_progress":{"agent":"claude-acp","state":{"missing":{"download_bytes":5}}}}"#,
        ),
    ];
    for (event, json) in cases {
        assert_eq!(serde_json::to_string(&event).unwrap(), json);
    }
}

#[test]
fn an_install_log_refuses_too_many_or_too_long_lines() {
    let lines = vec!["x".to_owned(); MAX_INSTALL_LOG_LINES + 1];
    let json = serde_json::to_string(&lines).unwrap();
    assert!(serde_json::from_str::<InstallLog>(&json).is_err());
    let long = vec!["x".repeat(MAX_INSTALL_LOG_LINE_BYTES + 1)];
    let json = serde_json::to_string(&long).unwrap();
    assert!(serde_json::from_str::<InstallLog>(&json).is_err());
    let fits = vec!["x".repeat(MAX_INSTALL_LOG_LINE_BYTES); MAX_INSTALL_LOG_LINES];
    let json = serde_json::to_string(&fits).unwrap();
    assert_eq!(
        serde_json::from_str::<InstallLog>(&json).unwrap().lines(),
        fits.as_slice()
    );
}

/// The codes 0048 adds, as the wire spells them.
const NEW_CODES: [(&str, ErrorCode); 11] = [
    ("agent.install_offline", ErrorCode::AgentInstallOffline),
    ("agent.install_blocked", ErrorCode::AgentInstallBlocked),
    ("agent.install_disk_full", ErrorCode::AgentInstallDiskFull),
    ("agent.install_failed", ErrorCode::AgentInstallFailed),
    ("signin.cancelled", ErrorCode::SigninCancelled),
    ("signin.link_expired", ErrorCode::SigninLinkExpired),
    ("signin.no_plan", ErrorCode::SigninNoPlan),
    ("signin.device_code_off", ErrorCode::SigninDeviceCodeOff),
    ("signin.port_busy", ErrorCode::SigninPortBusy),
    ("signin.failed", ErrorCode::SigninFailed),
    (
        "signin.policy_unreadable",
        ErrorCode::SigninPolicyUnreadable,
    ),
];

#[test]
fn the_install_and_sign_in_codes_decode() {
    for (text, code) in NEW_CODES {
        assert_eq!(code.as_str(), text);
        decodes_in_both(&format!("\"{text}\""), &code);
        round_trips(&code);
        let error = HostError::new(code, "detail");
        round_trips(&error);
        assert_eq!(error.retryable(), code.is_retryable());
        assert!(ErrorCode::ALL.contains(&code));
    }
    decodes_in_both("\"signin.made_up_later\"", &ErrorCode::Unknown);
    decodes_in_both("\"agent.install_later\"", &ErrorCode::Unknown);
    assert_eq!(ErrorCode::ALL.last(), Some(&ErrorCode::Unknown));
}
