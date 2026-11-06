//! The backend over `Agents` with every seam recorded (`fakes.rs`): the confirms that run
//! nothing when declined, the entry written on `Ready`, first run, the replay order,
//! `OpenSignInPage` and `SignOut` on a reused login, `main`'s first key, no key in a log, and the
//! two launchers; installs, keys, endpoints, sign-in bars and sign-in races in `tests/`.

mod after;
mod counts;
mod endpoint;
mod fakes;
mod identity;
mod identity_start;
mod key_marker;
mod key_reads;
mod key_state;
mod keys;
mod leave;
mod once;
mod quit;
mod races;
mod refusals;
mod retry;
mod reused;
mod signin;
mod switching;
mod takeover;
mod timing;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use clauseo_agents::testing::installed;
use clauseo_agents::{AgentsEvent, InstallState, SigninError, SigninState, Source};
use clauseo_process::SpawnOptions;
use clauseo_protocol::{
    AgentId, CommandOutcome, ErrorCode, InstallProgress, SetupEvent, SignIn, SignInMethod,
    SignInSource,
};
use clauseo_settings::AgentLaunch;
use serde_json::json;

use self::fakes::{HostCall, command, home, rig, setup_events};
use super::launch;
use crate::bridge::lock;
use crate::setup::confirm::Ask;
use crate::setup::start::launchers;
use crate::test_support::block_on;

const MAIN: &str = crate::MAIN_WINDOW;
const SETTINGS: &str = crate::settings_window::SETTINGS_WINDOW;
const KEY: &str = "sk-ant-api03-NEVER-IN-A-LOG-0b5e";
const CODE: &str = "PASTE-CODE-NEVER-LOGGED-7c1d";
const ENDPOINT_HOST: &str = "gw-never-logged-4e2a.firm.example";
const ENDPOINT: &str = "https://gw-never-logged-4e2a.firm.example";

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

#[test]
fn a_declined_install_or_endpoint_runs_nothing_and_answers_unauthorized() {
    let rig = rig(false);
    lock(&rig.agents.states).insert(
        claude(),
        InstallState::Missing {
            download_bytes: 41_500_001,
        },
    );
    let setup = rig.setup();
    let install = block_on(setup.run(MAIN, command("install_agent", &claude(), &json!({}))));
    assert_eq!(
        install.unwrap_err().code(),
        ErrorCode::TransportUnauthorized
    );
    let endpoint = command(
        "set_firm_endpoint",
        &claude(),
        &json!({ "url": "https://gateway.firm.example" }),
    );
    let endpoint = block_on(setup.run(SETTINGS, endpoint));
    assert_eq!(
        endpoint.unwrap_err().code(),
        ErrorCode::TransportUnauthorized
    );
    assert_eq!(rig.calls(), [], "no agents call");
    assert_eq!(rig.host.calls(), [], "no host call");
    assert_eq!(
        *lock(&rig.confirm.asked),
        [
            Ask::Install {
                name: "Claude Code".to_owned(),
                megabytes: Some(42)
            },
            Ask::Endpoint {
                name: "Claude Code".to_owned(),
                url: Some("https://gateway.firm.example".to_owned())
            }
        ]
    );
}

#[test]
fn an_accepted_install_and_endpoint_run_and_the_endpoint_shows_firm() {
    let rig = rig(true);
    let setup = rig.setup();
    let mut feed = rig.events.follow();
    let done = Ok(CommandOutcome::Done);
    assert_eq!(
        block_on(setup.run(MAIN, command("install_agent", &claude(), &json!({})))),
        done
    );
    let endpoint = command(
        "set_firm_endpoint",
        &claude(),
        &json!({ "url": "https://gateway.firm.example" }),
    );
    assert_eq!(block_on(setup.run(SETTINGS, endpoint)), done);
    assert_eq!(
        rig.calls(),
        [("install", claude()), ("cancel_sign_in", claude())],
        "the endpoint cancels whatever sign-in may be reserved"
    );
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::SetEndpoint(claude(), Some("https://gateway.firm.example".to_owned())),
            HostCall::RestartAgents(claude()),
            HostCall::LoginChanged(claude()),
        ],
        "the endpoint, then the agents restart with it and the limits read it"
    );
    assert!(matches!(
        setup_events(&mut feed).as_slice(),
        [SetupEvent::SignInChanged {
            state: SignIn::SignedIn {
                method: SignInMethod::Firm,
                ..
            },
            ..
        }]
    ));
}

#[test]
fn an_endpoint_that_is_not_https_is_refused_before_the_confirm() {
    let rig = rig(true);
    for url in [
        "http://gateway.firm.example",
        "https://user:pw@gateway.example",
        "nope",
    ] {
        let sent = command("set_firm_endpoint", &claude(), &json!({ "url": url }));
        let answer = block_on(rig.setup().run(SETTINGS, sent));
        assert_eq!(
            answer.unwrap_err().code(),
            ErrorCode::CommandInvalid,
            "{url}"
        );
    }
    assert_eq!(*lock(&rig.confirm.asked), []);
    assert_eq!(rig.host.calls(), []);
}

#[test]
fn ready_writes_the_entry_with_the_install_and_the_home_then_checks_status() {
    let rig = rig(true);
    let node = PathBuf::from("/data/agents/runtime/node-v24/bin/node");
    let folder = PathBuf::from("/data/agents/claude-acp/install/abc");
    let script = folder.join("node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js");
    lock(&rig.agents.installs).insert(
        claude(),
        installed(node.clone(), script.clone(), folder, "0.31.0"),
    );
    let ready = AgentsEvent::Install {
        agent: claude(),
        state: InstallState::Ready {
            version: "0.31.0".to_owned(),
        },
    };
    block_on(rig.backend.pump(futures::stream::iter([ready])));
    let home = home(&claude()).to_str().unwrap().to_owned();
    let want = AgentLaunch {
        command: node.to_str().unwrap().to_owned(),
        args: vec![script.to_str().unwrap().to_owned()],
        env: BTreeMap::from([
            ("ANTHROPIC_API_KEY".to_owned(), String::new()),
            ("CLAUDE_CONFIG_DIR".to_owned(), home.clone()),
            ("CLAUDE_SECURESTORAGE_CONFIG_DIR".to_owned(), home),
        ]),
    };
    assert_eq!(rig.host.calls(), [HostCall::SetAgentLaunch(claude(), want)]);
    assert_eq!(rig.calls(), [("check_status", claude())]);
}

#[test]
fn the_entry_s_env_follows_the_agent_and_the_login_source() {
    let installed = installed(
        PathBuf::from("/n/node"),
        PathBuf::from("/s/index.js"),
        PathBuf::from("/s"),
        "1",
    );
    let reused = launch(&claude(), &installed, Path::new("/h"), SignInSource::Reused).unwrap();
    assert_eq!(reused.env["CLAUDE_SECURESTORAGE_CONFIG_DIR"], "");
    assert_eq!(reused.env["CLAUDE_CONFIG_DIR"], "/h");
    let codex_launch = launch(&codex(), &installed, Path::new("/h"), SignInSource::Own).unwrap();
    assert_eq!(
        codex_launch.env.get("CODEX_API_KEY").map(String::as_str),
        Some("")
    );
    assert_eq!(
        codex_launch.env.get("HOME").map(String::as_str),
        (!cfg!(windows)).then_some("/h")
    );
    assert!(!codex_launch.env.contains_key("ANTHROPIC_API_KEY"));
}

fn missing(agent: AgentId) -> AgentsEvent {
    AgentsEvent::Install {
        agent,
        state: InstallState::Missing {
            download_bytes: 1_000,
        },
    }
}

#[test]
fn first_run_installs_claude_once_when_nothing_is_installed_or_managed() {
    let rig = rig(true);
    block_on(rig.backend.pump(futures::stream::iter([
        missing(claude()),
        missing(codex()),
        missing(codex()),
    ])));
    assert_eq!(rig.calls(), [("install", claude())]);

    let rig = self::rig(true);
    let managed = AgentsEvent::Install {
        agent: codex(),
        state: InstallState::Managed,
    };
    block_on(
        rig.backend
            .pump(futures::stream::iter([missing(claude()), managed])),
    );
    assert_eq!(rig.calls(), [], "a managed agent: no first run");
}

#[test]
fn a_new_follower_gets_setup_info_then_each_agent_s_install_then_sign_in() {
    let rig = rig(true);
    rig.events.publish(SetupEvent::SetupInfo {
        keys_available: true,
    });
    block_on(rig.backend.pump(futures::stream::iter([
        missing(claude()),
        AgentsEvent::Signin {
            agent: claude(),
            state: SigninState::Queued,
        },
    ])));
    let mut feed = rig.events.follow();
    assert_eq!(
        setup_events(&mut feed),
        [
            SetupEvent::SetupInfo {
                keys_available: true
            },
            SetupEvent::AgentInstallProgress {
                agent: claude(),
                state: InstallProgress::Missing {
                    download_bytes: clauseo_protocol::Count::clamped(1_000)
                },
            },
            SetupEvent::SignInChanged {
                agent: claude(),
                state: SignIn::Queued
            },
        ]
    );
}

#[test]
fn open_url_opens_and_open_sign_in_page_acts_only_on_a_device_code() {
    let rig = rig(true);
    let url = "https://claude.ai/oauth/authorize?code=true".to_owned();
    block_on(
        rig.backend
            .pump(futures::stream::iter([AgentsEvent::OpenUrl {
                agent: claude(),
                url: url.clone(),
            }])),
    );
    assert_eq!(*lock(&rig.opener.0), std::slice::from_ref(&url));
    let setup = rig.setup();
    let page = || command("open_sign_in_page", &codex(), &json!({}));
    assert_eq!(block_on(setup.run(MAIN, page())), Ok(CommandOutcome::Done));
    assert_eq!(
        lock(&rig.opener.0).len(),
        1,
        "no device code: nothing opens"
    );
    let device = "https://auth.openai.com/codex/device".to_owned();
    lock(&rig.agents.signin).insert(
        codex(),
        SigninState::DeviceCode {
            url: device.clone(),
            code: "ABCD-1234".to_owned(),
            expires_at: UNIX_EPOCH,
        },
    );
    assert_eq!(block_on(setup.run(MAIN, page())), Ok(CommandOutcome::Done));
    assert_eq!(*lock(&rig.opener.0), [url, device]);
}

#[test]
fn sign_out_on_a_reused_login_is_refused_and_runs_no_logout() {
    let rig = rig(true);
    lock(&rig.agents.signin).insert(
        claude(),
        SigninState::SignedIn {
            email: None,
            plan: None,
            source: Source::Reused,
            at: UNIX_EPOCH,
        },
    );
    let answer = block_on(
        rig.setup()
            .run(SETTINGS, command("sign_out", &claude(), &json!({}))),
    );
    assert_eq!(answer.unwrap_err().code(), ErrorCode::CommandInvalid);
    assert_eq!(rig.calls(), []);
}

#[test]
fn main_s_first_key_stores_once_shows_api_key_and_a_second_is_key_stored() {
    let rig = rig(true);
    let setup = rig.setup();
    let mut feed = rig.events.follow();
    let key = || command("set_agent_key", &claude(), &json!({ "key": KEY }));
    assert_eq!(block_on(setup.run(MAIN, key())), Ok(CommandOutcome::Done));
    assert!(rig.backend.keys.has(&claude()));
    let again = block_on(setup.run(MAIN, key())).unwrap_err();
    assert_eq!(again.code(), ErrorCode::SettingsKeyStored);
    assert!(matches!(
        setup_events(&mut feed).as_slice(),
        [SetupEvent::SignInChanged {
            state: SignIn::SignedIn {
                method: SignInMethod::ApiKey,
                ..
            },
            ..
        }]
    ));
    assert_eq!(
        block_on(setup.run(SETTINGS, command("clear_agent_key", &claude(), &json!({})))),
        Ok(CommandOutcome::Done)
    );
    assert!(!rig.backend.keys.has(&claude()));
}

/// A writer the capture layer writes every line into.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *lock(&self.0), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn no_key_reaches_a_log_line_an_event_or_a_debug() {
    let lines = Lines::default();
    let writer = lines.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let rig = rig(true);
    let setup = rig.setup();
    let mut feed = rig.events.follow();
    tracing::subscriber::with_default(subscriber, || {
        let key = || command("set_agent_key", &claude(), &json!({ "key": KEY }));
        let _first = block_on(setup.run(MAIN, key()));
        let _second = block_on(setup.run(MAIN, key()));
        let _replace = block_on(setup.run(SETTINGS, key()));
        let bad = command(
            "set_agent_key",
            &codex(),
            &json!({ "key": format!("{KEY} x") }),
        );
        let _refused = block_on(setup.run(SETTINGS, bad));
        let paste = command("paste_sign_in_code", &claude(), &json!({ "code": CODE }));
        let _pasted = block_on(setup.run(MAIN, paste));
        for url in [ENDPOINT.to_owned(), format!("{ENDPOINT}/?{CODE}")] {
            let endpoint = command("set_firm_endpoint", &codex(), &json!({ "url": url }));
            let _endpoint = block_on(setup.run(SETTINGS, endpoint));
        }
    });
    let text = String::from_utf8(lock(&lines.0).clone()).unwrap();
    assert!(
        text.contains("settings.key_stored") && text.contains("command.invalid"),
        "{text}"
    );
    for secret in [KEY, CODE, ENDPOINT_HOST] {
        assert!(!text.contains(secret), "{secret}: {text}");
    }
    let events = format!("{:?}", setup_events(&mut feed));
    for secret in [KEY, CODE, ENDPOINT_HOST] {
        assert!(!events.contains(secret), "{secret}: {events}");
    }
    assert!(!format!("{:?}", rig.backend.keys).contains(KEY));
    assert_eq!(
        rig.backend
            .keys
            .get(&claude())
            .as_deref()
            .map(String::as_str),
        Some(KEY)
    );
}

#[test]
fn the_sign_in_launcher_clears_the_environment_and_the_install_launcher_does_not() {
    let recorded: Mutex<Vec<SpawnOptions>> = Mutex::default();
    let home = PathBuf::from("/Users/lawyer");
    let (install, signin) = launchers(Some(home), |options| {
        let shown = format!("{options:?}");
        lock(&recorded).push(options);
        shown
    });
    assert_eq!(lock(&recorded).len(), 2);
    assert!(install.contains("clear_env: false"), "{install}");
    assert!(signin.contains("clear_env: true"), "{signin}");
    assert!(signin.contains("/Users/lawyer"), "{signin}");
    // Both end with the app on Linux (#386); elsewhere the option has no field to show.
    #[cfg(target_os = "linux")]
    for shown in [&install, &signin] {
        assert!(shown.contains("end_with_parent: true"), "{shown}");
    }
}

/// It proves something only on Linux, where `end_with_parent` is a field of the options;
/// elsewhere the option does nothing and the check passes trivially.
#[test]
fn the_host_s_launch_options_never_end_with_the_app() {
    // Its agents start on short-lived prep threads (`start.rs`): the option would kill them.
    let host = format!(
        "{:?}",
        crate::host_spawn_options(Some(PathBuf::from("/Users/lawyer")))
    );
    assert!(!host.contains("end_with_parent: true"), "{host}");
    assert!(host.contains("clear_env: true"), "{host}");
}

#[test]
fn a_refused_s18_call_answers_command_invalid() {
    let rig = rig(true);
    *lock(&rig.agents.refusal) = Some(SigninError::NotRunning(claude()));
    let answer = block_on(
        rig.setup()
            .run(MAIN, command("cancel_sign_in", &claude(), &json!({}))),
    );
    assert_eq!(answer.unwrap_err().code(), ErrorCode::CommandInvalid);
    assert_eq!(rig.calls(), [("cancel_sign_in", claude())]);
}
