//! A relaunch that fails: the agent exited mid-turn, the next prompt starts it again, and that
//! launch fails before its session attaches (the process exits on `session/load`, `session/new`
//! answers an error or the process exits on it, or the program does not start). The turn waiting
//! for the session ends failed with an `agent.*` code, and the thread takes the next prompt.

use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher};
use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId, ThreadStatus, TurnId};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Scenario, matter_with_thread};

/// What one launch does.
#[derive(Debug, Clone)]
enum Play {
    /// Starts `fake_agent` with this scenario.
    Agent(Scenario),
    /// Names a program that does not exist, so the OS refuses to start it.
    NoProgram,
}

/// A launcher that plays one [`Play`] per launch, in order, the last one again once they run
/// out; the spec's environment and folder are kept.
struct Sequence {
    inner: PassthroughLauncher,
    plays: Mutex<Vec<Play>>,
}

impl Sequence {
    fn new(dir: &Dir, plays: &[Play]) -> Arc<Self> {
        let mut plays = plays.to_vec();
        plays.reverse();
        Arc::new(Self {
            inner: dir.launcher(),
            plays: Mutex::new(plays),
        })
    }
}

impl Launcher for Sequence {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        let play = {
            let mut plays = self.plays.lock().unwrap();
            if plays.len() > 1 {
                plays.pop()
            } else {
                plays.last().cloned()
            }
        };
        let (program, args) = match play.expect("a play") {
            Play::Agent(scenario) => (
                spec.program().to_path_buf(),
                vec![OsString::from("--scenario"), OsString::from(scenario.arg())],
            ),
            Play::NoProgram => (spec.cwd().join("no-such-agent"), Vec::new()),
        };
        let spec = LaunchSpec::new(program, args, spec.env().clone(), spec.cwd().to_path_buf())?;
        self.inner.launch(&spec)
    }
}

/// `turn`'s `TurnFailed` code, if logged.
fn failed_code(events: &Events, turn: TurnId) -> Option<ErrorCode> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => Some(failed.code),
        _ => None,
    })
}

/// Turn 1 on the agent `first` exits mid-turn; turn 2 relaunches it as `then`, which fails
/// before its session attaches. Turn 2 must end failed with one of `codes`, the thread must
/// not stay running, and a third prompt (which relaunches `then` again) must be accepted and
/// end the same way.
fn relaunch_fails(test: &str, first: Play, then: Play, codes: &[ErrorCode]) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Bundled("idle"))]);
    let host = dir.start_launching(0, Sequence::new(&dir, &[first, then]), |config| config);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));

    let (first_turn, _) = host.prompt(thread, "Start the answer.");
    let ended = events.until_turn_ended(first_turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    assert_eq!(
        failed_code(&events, first_turn),
        Some(ErrorCode::AgentExited),
        "{}",
        events.dump()
    );

    for prompt in ["Try again.", "And once more."] {
        let (turn, _) = host.prompt(thread, prompt);
        let ended = events.until_turn_ended(turn);
        assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
        let code = failed_code(&events, turn);
        assert!(
            code.is_some_and(|code| codes.contains(&code)),
            "turn {turn} failed with {code:?}: {}",
            events.dump()
        );
        host.settle();
        let status = host.status(matter, thread);
        assert!(
            !matches!(status, ThreadStatus::Running | ThreadStatus::Starting),
            "the thread is still {status:?} after a failed relaunch: {}",
            events.dump()
        );
    }
    host.shutdown();
}

/// The report's case: the relaunched `exit-mid-turn` agent expects `session/new`, gets
/// `session/load` (resume), and exits 3 on the mismatch.
#[test]
fn relaunch_exiting_on_session_load_ends_the_turn() {
    let agent = Play::Agent(Scenario::Bundled("exit-mid-turn"));
    relaunch_fails(
        "relaunch_exiting_on_session_load_ends_the_turn",
        agent.clone(),
        agent,
        &[ErrorCode::AgentExited],
    );
}

/// No resume: the relaunched agent answers `session/new` with an error, which reaches the turn
/// as `agent.protocol_error` (the code of any error answer from the agent), as on a first launch.
#[test]
fn relaunch_whose_session_new_errors_ends_the_turn() {
    relaunch_fails(
        "relaunch_whose_session_new_errors_ends_the_turn",
        Play::Agent(Scenario::Local("exit-mid-turn-no-load")),
        Play::Agent(Scenario::Local("new-session-error")),
        &[ErrorCode::AgentProtocolError],
    );
}

/// No resume: the relaunched agent exits on `session/new` without answering.
#[test]
fn relaunch_exiting_on_session_new_ends_the_turn() {
    relaunch_fails(
        "relaunch_exiting_on_session_new_ends_the_turn",
        Play::Agent(Scenario::Local("exit-mid-turn-no-load")),
        Play::Agent(Scenario::Local("new-session-exit")),
        &[ErrorCode::AgentExited],
    );
}

/// The relaunch's program does not start at all.
#[test]
fn relaunch_that_cannot_spawn_ends_the_turn() {
    relaunch_fails(
        "relaunch_that_cannot_spawn_ends_the_turn",
        Play::Agent(Scenario::Bundled("exit-mid-turn")),
        Play::NoProgram,
        &[ErrorCode::AgentLaunchFailed],
    );
}
