//! The `setup` command's path: the allow-lists, `main`'s first key, the sign-in join, the events
//! on the windows' channels in order and in fragments, and no secret in a log or a `Debug`.

use std::sync::{Arc, Mutex};

use clauseo_protocol::{
    AgentId, CommandOutcome, Count, ErrorCode, HostError, InstallLog, InstallProgress,
    MAX_INSTALL_LOG_LINE_BYTES, MAX_INSTALL_LOG_LINES, SetupAction, SetupCommand, SetupEvent,
    SignIn, Timestamp,
};
use serde_json::{Value, json};

use super::admit::{self, key_stored};
use super::*;
use crate::bridge::Bridge;
use crate::host_start::HostCell;
use crate::test_support::{Messages, Scratch, block_on, collector, open};

const MAIN: &str = crate::MAIN_WINDOW;
const SETTINGS: &str = crate::settings_window::SETTINGS_WINDOW;

/// The pasted code and the key the secret tests send.
const SECRET: &str = "sk-ant-api03-NEVER-IN-A-LOG-7f3a";

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

/// The command for `action` for Claude, its secrets [`SECRET`], decoded as a window's JSON is.
fn command(action: SetupAction) -> SetupCommand {
    let agent = json!({ "agent": "claude-acp" });
    let with = |extra: Value| {
        let mut fields = agent.clone();
        fields
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        fields
    };
    let (name, fields) = match action {
        SetupAction::InstallAgent => ("install_agent", agent),
        SetupAction::RetryAgentInstall => ("retry_agent_install", agent),
        SetupAction::StartSignIn => ("start_sign_in", agent),
        SetupAction::CancelSignIn => ("cancel_sign_in", agent),
        SetupAction::PasteSignInCode => ("paste_sign_in_code", with(json!({ "code": SECRET }))),
        SetupAction::UseDeviceCode => ("use_device_code", agent),
        SetupAction::OpenSignInPage => ("open_sign_in_page", agent),
        SetupAction::SignOut => ("sign_out", agent),
        SetupAction::UseFoundLogin => ("use_found_login", agent),
        SetupAction::UseDifferentAccount => ("use_different_account", agent),
        SetupAction::StopUsingLogin => ("stop_using_login", agent),
        SetupAction::FindLogin => ("find_login", agent),
        SetupAction::SetAgentKey => ("set_agent_key", with(json!({ "key": SECRET }))),
        SetupAction::ClearAgentKey => ("clear_agent_key", agent),
        SetupAction::SetFirmEndpoint => ("set_firm_endpoint", agent),
    };
    let command: SetupCommand = serde_json::from_value(json!({ name: fields })).unwrap();
    assert_eq!(command.action(), action);
    command
}

/// The actions `main` may send: 0048's shared list, and a first key when allowed.
fn mains(first_key: bool) -> Vec<SetupAction> {
    let mut list = vec![
        SetupAction::InstallAgent,
        SetupAction::RetryAgentInstall,
        SetupAction::StartSignIn,
        SetupAction::CancelSignIn,
        SetupAction::PasteSignInCode,
        SetupAction::UseDeviceCode,
        SetupAction::OpenSignInPage,
        SetupAction::UseFoundLogin,
        SetupAction::UseDifferentAccount,
    ];
    if first_key {
        list.push(SetupAction::SetAgentKey);
    }
    list
}

/// A setup run by a stub that answers every action `Done` with no event.
fn stubbed(first_key: bool) -> (Setup, Arc<StubBackend>) {
    let stub = Arc::new(StubBackend::default());
    for action in SetupAction::ALL {
        stub.script(action, Ok(CommandOutcome::Done), Vec::new());
    }
    let backend: Arc<dyn SetupBackend> = Arc::clone(&stub) as Arc<dyn SetupBackend>;
    (Setup::with_rules(backend, first_key), stub)
}

#[test]
fn every_action_is_allowed_in_exactly_its_windows_with_either_key_rule() {
    for first_key in [true, false] {
        let main = mains(first_key);
        for action in SetupAction::ALL {
            let in_main = admit::admit(first_key, MAIN, action);
            assert_eq!(
                in_main.is_ok(),
                main.contains(&action),
                "{action:?} {first_key}"
            );
            // The settings window sends every action, a key replacing the one stored.
            assert_eq!(
                admit::admit(first_key, SETTINGS, action),
                Ok(KeyWrite::Replace)
            );
            for label in ["counts", "e2e", "other", ""] {
                let error = admit::admit(first_key, label, action).unwrap_err();
                assert_eq!(error.code(), ErrorCode::TransportUnauthorized, "{label}");
            }
            if let Err(error) = in_main {
                assert_eq!(error.code(), ErrorCode::TransportUnauthorized);
            }
        }
        let key = admit::admit(first_key, MAIN, SetupAction::SetAgentKey);
        assert_eq!(
            key.is_ok_and(|write| write == KeyWrite::OnlyIfAbsent),
            first_key
        );
    }
}

#[test]
fn a_refused_action_never_reaches_the_backend() {
    let (setup, stub) = stubbed(false);
    for (label, action) in [
        (MAIN, SetupAction::SignOut),
        (MAIN, SetupAction::SetAgentKey),
        ("counts", SetupAction::InstallAgent),
    ] {
        let error = block_on(setup.run(label, command(action))).unwrap_err();
        assert_eq!(error.code(), ErrorCode::TransportUnauthorized);
    }
    assert_eq!(stub.calls(), []);
}

#[test]
fn main_stores_a_first_key_only_and_settings_replaces_it() {
    let stub = Arc::new(StubBackend::default());
    for action in [SetupAction::SetAgentKey, SetupAction::ClearAgentKey] {
        stub.script(action, Ok(CommandOutcome::Done), Vec::new());
    }
    // The default rules: the ruling lets main store a first key.
    let setup = Setup::new(Arc::clone(&stub) as Arc<dyn SetupBackend>);
    let set = || command(SetupAction::SetAgentKey);
    assert_eq!(block_on(setup.run(MAIN, set())), Ok(CommandOutcome::Done));
    assert!(stub.has_key(&claude()));
    let error = block_on(setup.run(MAIN, set())).unwrap_err();
    assert_eq!(error, key_stored(&claude()));
    assert_eq!(error.code(), ErrorCode::SettingsKeyStored);
    assert!(!error.message().contains(SECRET));
    assert_eq!(
        block_on(setup.run(SETTINGS, set())),
        Ok(CommandOutcome::Done)
    );
    // Clearing stays the settings window's; after it, main may store a first key again.
    let clear = command(SetupAction::ClearAgentKey);
    assert!(block_on(setup.run(MAIN, clear)).is_err());
    let clear = command(SetupAction::ClearAgentKey);
    assert_eq!(
        block_on(setup.run(SETTINGS, clear)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(block_on(setup.run(MAIN, set())), Ok(CommandOutcome::Done));
}

#[test]
fn two_keys_from_main_at_once_store_exactly_one() {
    for _ in 0..50 {
        let (setup, stub) = stubbed(true);
        let answers: Vec<Result<CommandOutcome, HostError>> = std::thread::scope(|scope| {
            let sends: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| block_on(setup.run(MAIN, command(SetupAction::SetAgentKey))))
                })
                .collect();
            sends.into_iter().map(|send| send.join().unwrap()).collect()
        });
        let stored = answers.iter().filter(|answer| answer.is_ok()).count();
        assert_eq!(stored, 1, "{answers:?}");
        let refused = answers
            .iter()
            .find_map(|answer| answer.clone().err())
            .unwrap();
        assert_eq!(refused.code(), ErrorCode::SettingsKeyStored);
        assert!(stub.has_key(&claude()));
    }
}

fn sign_in(state: SignIn) -> SetupEvent {
    SetupEvent::SignInChanged {
        agent: claude(),
        state,
    }
}

#[test]
fn a_second_start_sign_in_joins_the_running_one() {
    let stub = Arc::new(StubBackend::default());
    stub.script(
        SetupAction::StartSignIn,
        Ok(CommandOutcome::Done),
        vec![sign_in(SignIn::Queued)],
    );
    let setup = Setup::new(Arc::clone(&stub) as Arc<dyn SetupBackend>);
    let start = || command(SetupAction::StartSignIn);
    assert_eq!(block_on(setup.run(MAIN, start())), Ok(CommandOutcome::Done));
    // From either window, while it runs: `Done`, and the backend is not called.
    assert_eq!(
        block_on(setup.run(SETTINGS, start())),
        Ok(CommandOutcome::Done)
    );
    let waiting = SignIn::Waiting {
        since: Timestamp::new(1_760_000_000_000).unwrap(),
        nudge: false,
    };
    setup.events.publish(sign_in(waiting));
    assert_eq!(block_on(setup.run(MAIN, start())), Ok(CommandOutcome::Done));
    let starts = |stub: &StubBackend| {
        let calls = stub.calls();
        calls
            .iter()
            .filter(|(action, _)| *action == SetupAction::StartSignIn)
            .count()
    };
    assert_eq!(starts(&stub), 1);
    // Once it stopped, a new one starts.
    setup
        .events
        .publish(sign_in(SignIn::SignedOut { found: None }));
    assert_eq!(block_on(setup.run(MAIN, start())), Ok(CommandOutcome::Done));
    assert_eq!(starts(&stub), 2);
}

#[test]
fn a_start_with_the_backend_is_joined_and_the_mark_goes_with_its_answer() {
    let (setup, stub) = stubbed(true);
    let starting = setup.start_sign_in(&claude()).unwrap();
    let start = || command(SetupAction::StartSignIn);
    assert_eq!(block_on(setup.run(MAIN, start())), Ok(CommandOutcome::Done));
    assert_eq!(stub.calls(), []);
    drop(starting);
    assert_eq!(block_on(setup.run(MAIN, start())), Ok(CommandOutcome::Done));
    assert_eq!(stub.calls(), vec![(SetupAction::StartSignIn, claude())]);
}

/// A writer the tests' tracing subscriber writes every line into.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *crate::bridge::lock(&self.0), buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Lines {
    fn text(&self) -> String {
        String::from_utf8(crate::bridge::lock(&self.0).clone()).unwrap()
    }
}

#[test]
fn no_secret_reaches_a_log_line_or_a_debug() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let (setup, stub) = stubbed(true);
    tracing::subscriber::with_default(subscriber, || {
        for label in [MAIN, MAIN, SETTINGS, "counts"] {
            for action in [SetupAction::PasteSignInCode, SetupAction::SetAgentKey] {
                let sent = command(action);
                assert!(!format!("{sent:?} {sent:#?}").contains(SECRET));
                let _answer = block_on(setup.run(label, sent));
            }
        }
        events::note(&claude(), "claude_reuse_fallback");
    });
    let text = lines.text();
    assert!(
        text.contains("setup") && text.contains("PasteSignInCode"),
        "{text}"
    );
    assert!(
        text.contains("settings.key_stored"),
        "the second key's code: {text}"
    );
    assert!(text.contains("claude_reuse_fallback"), "{text}");
    assert!(!text.contains(SECRET), "{text}");
    assert!(!format!("{:?}", stub.calls()).contains(SECRET));
}

/// The setup event in the channel message `text`.
fn setup_event(text: &str) -> SetupEvent {
    let mut message: Value = serde_json::from_str(text).unwrap();
    serde_json::from_value(message["setup"].take()).unwrap_or_else(|err| panic!("{err}: {text}"))
}

/// The next `setup` message on `messages`, whole or put back together from its fragments as
/// the transport does. The app feed's own messages (`settings_changed`, the lists), all whole
/// here, are skipped.
fn next_setup(messages: &Messages) -> SetupEvent {
    let first = loop {
        let text = messages.next_text();
        if text.starts_with(r#"{"setup":"#) || text.starts_with(r#"{"fragment":"#) {
            break text;
        }
    };
    let value: Value = serde_json::from_str(&first).unwrap();
    let Some(fragment) = value.get("fragment") else {
        assert!(first.len() < crate::frame::FRAME_LIMIT);
        return setup_event(&first);
    };
    let count = fragment["count"].as_u64().unwrap();
    let id = fragment["id"].clone();
    let mut joined = fragment["text"].as_str().unwrap().to_owned();
    for index in 1..count {
        let text = messages.next_text();
        assert!(text.len() < crate::frame::FRAME_LIMIT);
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["fragment"]["id"], id);
        assert_eq!(value["fragment"]["index"], index);
        joined.push_str(value["fragment"]["text"].as_str().unwrap());
    }
    setup_event(&joined)
}

fn install(state: InstallProgress) -> SetupEvent {
    SetupEvent::AgentInstallProgress {
        agent: claude(),
        state,
    }
}

#[test]
fn setup_reaches_the_backend_and_its_events_stream_to_both_windows_in_order() {
    let scratch = Scratch::new("setup-events", &[]);
    let host = scratch.start(Vec::new());
    let stub = Arc::new(StubBackend::default());
    let progress = vec![
        install(InstallProgress::Downloading {
            received: Count::ZERO,
            total: Count::from(1_000_u32),
        }),
        install(InstallProgress::Downloading {
            received: Count::from(1_000_u32),
            total: Count::from(1_000_u32),
        }),
        install(InstallProgress::Installing),
        install(InstallProgress::Ready {
            version: "0.89.1".to_owned(),
        }),
    ];
    stub.script(
        SetupAction::InstallAgent,
        Ok(CommandOutcome::Done),
        progress.clone(),
    );
    let setup = Setup::new(Arc::clone(&stub) as Arc<dyn SetupBackend>);
    let events = setup.events();
    let bridge = Bridge::with_setup(HostCell::ready(host.host.clone()), setup);
    // Published before the windows follow: each gets the latest first.
    events.publish(SetupEvent::SetupInfo {
        keys_available: false,
    });
    events.publish(sign_in(SignIn::SignedOut { found: None }));
    let main = open(&bridge, MAIN);
    let settings = open(&bridge, SETTINGS);
    // Another label gets no limits view, so no first message to wait for.
    let (other_channel, other) = collector();
    block_on(bridge.open_stream("other", other_channel)).unwrap();
    for label in [MAIN, SETTINGS, "other"] {
        block_on(bridge.subscribe_app(label)).unwrap();
    }
    for window in [&main, &settings] {
        assert_eq!(
            next_setup(window),
            SetupEvent::SetupInfo {
                keys_available: false
            }
        );
        assert_eq!(
            next_setup(window),
            sign_in(SignIn::SignedOut { found: None })
        );
    }
    let answer = block_on(bridge.setup(MAIN, command(SetupAction::InstallAgent)));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(stub.calls(), vec![(SetupAction::InstallAgent, claude())]);
    for window in [&main, &settings] {
        for event in &progress {
            assert_eq!(&next_setup(window), event);
        }
    }
    // A window that may send no setup command follows none of its events.
    assert_eq!(other.drain(), Vec::<String>::new());
    // And the bridge refuses it before the backend.
    let refused = block_on(bridge.setup("other", command(SetupAction::InstallAgent)));
    assert_eq!(
        refused.unwrap_err().code(),
        ErrorCode::TransportUnauthorized
    );
    assert_eq!(stub.calls().len(), 1);
    // The bridge's windows go before the host stops.
    drop(bridge);
    drop(host);
}

#[test]
fn a_large_failed_install_arrives_in_fragments_and_whole() {
    let scratch = Scratch::new("setup-fragments", &[]);
    let stub = Arc::new(StubBackend::default());
    let line = "x\"\\".repeat((MAX_INSTALL_LOG_LINE_BYTES - 8) / 3);
    let log = InstallLog::new((0..MAX_INSTALL_LOG_LINES).map(|n| format!("{n} {line}")));
    let failed = install(InstallProgress::Failed {
        code: ErrorCode::AgentInstallFailed,
        need_mb: None,
        free_mb: None,
        hosts: vec!["registry.npmjs.org".to_owned()],
        log,
    });
    let text = crate::frame::setup_text(failed.clone()).unwrap();
    assert!(
        text.len() > 2 * crate::frame::FRAME_LIMIT,
        "{} bytes",
        text.len()
    );
    stub.script(
        SetupAction::RetryAgentInstall,
        Ok(CommandOutcome::Done),
        vec![failed.clone(), install(InstallProgress::Paused)],
    );
    let host = scratch.start(Vec::new());
    let setup = Setup::new(Arc::clone(&stub) as Arc<dyn SetupBackend>);
    let bridge = Bridge::with_setup(HostCell::ready(host.host.clone()), setup);
    let main = open(&bridge, MAIN);
    let settings = open(&bridge, SETTINGS);
    block_on(bridge.subscribe_app(MAIN)).unwrap();
    block_on(bridge.subscribe_app(SETTINGS)).unwrap();
    let answer = block_on(bridge.setup(SETTINGS, command(SetupAction::RetryAgentInstall)));
    assert_eq!(answer, Ok(CommandOutcome::Done));
    for window in [&main, &settings] {
        assert_eq!(next_setup(window), failed);
        assert_eq!(next_setup(window), install(InstallProgress::Paused));
    }
}

#[test]
fn not_wired_answers_internal_and_publishes_only_that_keys_are_not_offered() {
    let setup = Setup::not_wired();
    let feed = setup.events().follow();
    let error = block_on(setup.run(MAIN, command(SetupAction::InstallAgent))).unwrap_err();
    assert_eq!(error.code(), ErrorCode::Internal);
    drop(setup);
    let published = block_on(futures::StreamExt::collect::<Vec<_>>(feed));
    // Its one event, from its start: no key screens.
    assert_eq!(
        published
            .iter()
            .map(|text| setup_event(text))
            .collect::<Vec<_>>(),
        [SetupEvent::SetupInfo {
            keys_available: false
        }]
    );
}

#[test]
fn a_late_follower_gets_info_then_each_agent_s_latest_states() {
    let events = SetupEvents::default();
    let codex = AgentId::new("codex-acp").unwrap();
    events.publish(SetupEvent::SignInChanged {
        agent: codex.clone(),
        state: SignIn::Queued,
    });
    events.publish(install(InstallProgress::Installing));
    events.publish(install(InstallProgress::Ready {
        version: "1".to_owned(),
    }));
    events.publish(SetupEvent::SetupInfo {
        keys_available: true,
    });
    events.publish(sign_in(SignIn::Queued));
    let mut feed = events.follow();
    let got: Vec<SetupEvent> = std::iter::from_fn(|| feed.try_recv().ok())
        .map(|text| setup_event(&text))
        .collect();
    assert_eq!(
        got,
        vec![
            SetupEvent::SetupInfo {
                keys_available: true
            },
            install(InstallProgress::Ready {
                version: "1".to_owned()
            }),
            sign_in(SignIn::Queued),
            SetupEvent::SignInChanged {
                agent: codex.clone(),
                state: SignIn::Queued,
            },
        ]
    );
    assert!(events.sign_in_running(&codex) && events.sign_in_running(&claude()));
}
