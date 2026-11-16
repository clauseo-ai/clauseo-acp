//! A setup command answers while an install runs. The pump follows `Agents`' events on its own
//! thread, as `clauseo-setup` does; `Agents::install` only sends the worker a message, and the
//! install's events only publish. So with the pump parked between a slow install's events, or
//! held in the entry write on `Ready` (the one host call it waits on), `cancel_sign_in`, a
//! second `install_agent` and `start_sign_in` (which takes the sign-in gate) still answer.
//! Each must answer within a second, a bound far above the microseconds they take, so no slow
//! machine fails it; a lock shared with the pump would hold them for as long as the install.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use clauseo_agents::testing::installed;
use clauseo_agents::{AgentsEvent, InstallState};
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, HostError, SetupCommand};
use clauseo_settings::EntryWrite;
use futures::channel::{mpsc, oneshot};
use serde_json::json;

use super::fakes::{Rig, command, rig};
use super::{MAIN, claude, codex};
use crate::bridge::lock;
use crate::test_support::block_on;

/// `RecordingHost::launch_gate`: told when the entry write starts, then opened by the test.
pub(super) type LaunchGate = (std::sync::mpsc::Sender<()>, oneshot::Receiver<()>);

/// The fake host's entry write, held at `gate` when a test set one.
pub(super) async fn held_write(gate: Option<LaunchGate>) -> Result<EntryWrite, HostError> {
    if let Some((started, gate)) = gate {
        let _held = (started.send(()), gate.await);
    }
    Ok(EntryWrite::Written)
}

/// The bound on each answer.
const BOUND: Duration = Duration::from_secs(1);

fn install_event(agent: AgentId, state: InstallState) -> AgentsEvent {
    AgentsEvent::Install { agent, state }
}

/// `sent`'s answer from a fresh `Setup` on its own thread, which must come within [`BOUND`].
fn answers_in_time(rig: &Rig, sent: SetupCommand, what: &str) -> Result<CommandOutcome, HostError> {
    let setup = rig.setup();
    let (tell, told) = std::sync::mpsc::channel();
    let caller = std::thread::spawn(move || {
        drop(tell.send(block_on(setup.run(MAIN, sent))));
    });
    let answer = match told.recv_timeout(BOUND) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => panic!("{what}: no answer within {BOUND:?}"),
        Err(RecvTimeoutError::Disconnected) => panic!("{what}: the caller panicked"),
    };
    caller.join().unwrap();
    answer
}

/// The commands, each answering within [`BOUND`]: `cancel_sign_in` with none running (a read of
/// the sign-in state, refused), `install_agent` for Codex and, with `sign_in`, `start_sign_in`
/// for Codex (only once: a second would join the first without reaching the backend).
fn all_answer(rig: &Rig, phase: &str, sign_in: bool) {
    let cancel = command("cancel_sign_in", &claude(), &json!({}));
    let refused = answers_in_time(rig, cancel, &format!("cancel, {phase}"));
    assert_eq!(
        refused.unwrap_err().code(),
        ErrorCode::CommandInvalid,
        "{phase}"
    );
    let install = command("install_agent", &codex(), &json!({}));
    let done = Ok(CommandOutcome::Done);
    assert_eq!(
        answers_in_time(rig, install, &format!("install, {phase}")),
        done
    );
    if sign_in {
        let start = command("start_sign_in", &codex(), &json!({}));
        assert_eq!(
            answers_in_time(rig, start, &format!("sign-in, {phase}")),
            done
        );
    }
}

#[test]
fn setup_commands_answer_within_a_second_while_an_install_runs() {
    let rig = rig(true);
    let folder = PathBuf::from("/data/agents/claude-acp/install/abc");
    lock(&rig.agents.installs).insert(
        claude(),
        installed(
            PathBuf::from("/data/node"),
            folder.join("index.js"),
            folder,
            "0.31.0",
        ),
    );
    let (open, gate) = oneshot::channel();
    let (started, writing) = std::sync::mpsc::channel();
    *lock(&rig.host.launch_gate) = Some((started, gate));
    let (feed, source) = mpsc::unbounded();
    let backend = Arc::clone(&rig.backend);
    let pump = std::thread::Builder::new()
        .name("clauseo-setup".to_owned())
        .spawn(move || block_on(backend.pump(source)))
        .unwrap();

    // A slow install between its events: the pump waits for the next one.
    for state in [
        InstallState::Downloading {
            received: 1,
            total: 1_000_000,
        },
        InstallState::Installing,
    ] {
        feed.unbounded_send(install_event(claude(), state)).unwrap();
    }
    all_answer(&rig, "installing", false);

    // `Ready`: the pump is held in the entry write until the gate opens.
    let ready = InstallState::Ready {
        version: "0.31.0".to_owned(),
    };
    feed.unbounded_send(install_event(claude(), ready)).unwrap();
    writing
        .recv_timeout(Duration::from_secs(30))
        .expect("the pump never wrote the entry");
    all_answer(&rig, "writing the entry", true);

    open.send(()).unwrap();
    drop(feed);
    pump.join().unwrap();
    assert!(
        rig.calls().contains(&("sign_in", codex())),
        "{:?}",
        rig.calls()
    );
    assert_eq!(
        rig.calls()
            .iter()
            .filter(|(name, _)| *name == "install")
            .count(),
        2,
        "the installs reached Agents: {:?}",
        rig.calls()
    );
}
