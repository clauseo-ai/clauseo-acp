//! The leave order on the recording fakes, in exact sequence: `StopUsingLogin` on a reused
//! Codex login, a stop that times out (the leave stops there with nothing changed),
//! `UseFoundLogin`'s order, and a split found in temp homes running the leave with its reason.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use clauseo_agents::testing::installed;
use clauseo_agents::{Found, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{CommandOutcome, ErrorCode};
use serde_json::json;

use super::fakes::{HostCall, Rig, command, rig};
use super::{SETTINGS, codex};
use crate::bridge::lock;

fn signed_in(source: Source) -> SigninState {
    SigninState::SignedIn {
        email: Some("lawyer@firm.example".to_owned()),
        plan: None,
        source,
        at: UNIX_EPOCH,
    }
}

/// A rig whose Codex is installed and signed in with the lawyer's own login, reused.
fn reused_codex() -> Rig {
    let rig = rig(true);
    let folder = PathBuf::from("/data/agents/codex-acp/install/abc");
    let script = folder.join("node_modules/@agentclientprotocol/codex-acp/dist/index.js");
    lock(&rig.agents.installs).insert(
        codex(),
        installed(PathBuf::from("/data/node"), script, folder, "0.9.0"),
    );
    lock(&rig.agents.signin).insert(codex(), signed_in(Source::Reused));
    rig.backend.wiring.reused.set(&codex(), true);
    rig.backend
        .compose
        .signin(&codex(), signed_in(Source::Reused));
    rig
}

fn stop_using(rig: &Rig) -> Result<CommandOutcome, clauseo_protocol::HostError> {
    let setup = rig.setup();
    let send = setup.run(SETTINGS, command("stop_using_login", &codex(), &json!({})));
    rig.pumped(send)
}

const LEAVE: [&str; 9] = [
    "readers.hold",
    "host.stop_agents",
    "agents.stop_using_login",
    "host.set_reused_login",
    "host.remove_login_links",
    "host.set_agent_launch",
    "readers.set_entry",
    "readers.release",
    "host.login_changed",
];

#[test]
fn stop_using_login_runs_the_leave_order_in_exact_sequence() {
    let rig = reused_codex();
    lock(&rig.agents.answers).insert("stop_using_login", SigninState::SignedOut { found: None });
    assert_eq!(stop_using(&rig), Ok(CommandOutcome::Done));
    assert_eq!(rig.journal(), LEAVE);
    let entries = lock(&rig.readers.entries).clone();
    assert!(matches!(
        entries.as_slice(),
        [(clauseo_protocol::AgentKind::Codex, Some(_))]
    ));
    let calls = rig.host.calls();
    assert_eq!(calls[1], HostCall::SetReusedLogin(codex(), false));
    assert!(!rig.backend.wiring.reused.get(&codex()));
    // The entry is rewritten as Clauseo's own; no restart until a new login is in place.
    assert!(!calls.contains(&HostCall::RestartAgents(codex())));
}

#[test]
fn a_stop_that_times_out_lifts_the_hold_changes_nothing_and_answers_still_running() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let error = stop_using(&rig).unwrap_err();
    assert_eq!(error.code(), ErrorCode::SigninAgentsStillRunning);
    assert!(error.code().is_retryable(), "the lawyer may try again");
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::StopAgents(codex()),
            HostCall::RestartAgents(codex())
        ],
        "stop_agents, then restart_agents, and nothing after"
    );
    assert_eq!(
        rig.journal(),
        [
            "readers.hold",
            "host.stop_agents",
            "host.restart_agents",
            "readers.release"
        ]
    );
    // The old login stays: no S18 call, still reused everywhere.
    assert_eq!(rig.calls(), []);
    assert!(rig.backend.wiring.reused.get(&codex()));
    assert_eq!(
        rig.backend.compose.s18(&codex()),
        Some(signed_in(Source::Reused))
    );
}

#[test]
fn use_found_login_tells_the_host_then_writes_the_entry_then_restarts() {
    let rig = rig(true);
    let folder = PathBuf::from("/data/agents/codex-acp/install/abc");
    let script = folder.join("node_modules/@agentclientprotocol/codex-acp/dist/index.js");
    lock(&rig.agents.installs).insert(
        codex(),
        installed(PathBuf::from("/data/node"), script, folder, "0.9.0"),
    );
    let found = SigninState::LoginFound {
        from: clauseo_agents::LoginFrom::Codex,
        email: None,
    };
    lock(&rig.agents.signin).insert(codex(), found.clone());
    rig.backend.compose.signin(&codex(), found);
    lock(&rig.agents.answers).insert("use_found_login", signed_in(Source::Reused));
    let setup = rig.setup();
    let send = setup.run(SETTINGS, command("use_found_login", &codex(), &json!({})));
    assert_eq!(rig.pumped(send), Ok(CommandOutcome::Done));
    assert_eq!(
        rig.journal(),
        [
            "readers.hold",
            "agents.use_found_login",
            "host.set_reused_login",
            "host.set_agent_launch",
            "readers.set_entry",
            "readers.release",
            "host.restart_agents",
            "host.login_changed",
        ]
    );
    assert_eq!(rig.host.calls()[0], HostCall::SetReusedLogin(codex(), true));
    assert!(rig.backend.wiring.reused.get(&codex()));
}

/// The split, end to end on the desktop side: temp homes with fake, non-credential files at
/// mode 0000 (any open would fail), the link made then split by a rename-save, the hook's
/// check finding the split, then the leave order with S18's reason. S18's own unlink is
/// S18's (its tests); here `stop_using_login` is the fake's.
#[cfg(unix)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "test set-up writes fake login files; the desktop crate has no write module"
)]
fn a_split_found_in_temp_homes_runs_the_leave_order_with_its_reason() {
    use std::os::unix::fs::PermissionsExt as _;

    use clauseo_host::{LoginCheck as _, LoginLink};

    use crate::setup::login_check::DesktopLoginCheck;
    use crate::test_support::Scratch;

    let scratch = Scratch::folder("leave-split", &["home", "lawyer/.codex"]);
    let (home, lawyer) = (scratch.path().join("home"), scratch.path().join("lawyer"));
    let lawyers = lawyer.join(".codex").join("auth.json");
    let fake = |path: &std::path::Path| {
        std::fs::write(path, b"not a credential").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    };
    fake(&lawyers);
    std::fs::hard_link(&lawyers, home.join("auth.json")).unwrap();
    let rig = reused_codex();
    let check = DesktopLoginCheck::new(Some(lawyer), rig.backend.wiring.reused.clone());
    assert_eq!(check.check(&codex(), &home), LoginLink::Shared);
    fake(&home.join("auth.json.tmp"));
    std::fs::rename(home.join("auth.json.tmp"), home.join("auth.json")).unwrap();
    assert_eq!(check.check(&codex(), &home), LoginLink::Split);
    let ended = SigninState::SignedOut {
        found: Some(Found::CodexLinkSplit),
    };
    lock(&rig.agents.answers).insert("stop_using_login", ended);
    let agent = codex();
    let leave =
        crate::setup::leave::leave_reused(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    assert_eq!(rig.pumped(leave), Ok(()));
    assert_eq!(rig.journal(), LEAVE);
    // Desktop's word is now "not reused": a launch's check is Clauseo's own login's.
    assert_eq!(check.check(&codex(), &home), LoginLink::Own);
}
