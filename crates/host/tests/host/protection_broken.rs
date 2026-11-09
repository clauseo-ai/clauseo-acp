//! R17 PR-E review: a set-up in place but broken fails closed, refusing every launch with
//! `sandbox.blocked` whatever the settings say, and its cause stays `SetUpFailed` for the app's
//! words; the limits reader, which runs the agent's program outside the jail, starts only when
//! a launch would; and a turn already running when the computer's cause turns `Blocked`
//! finishes, then its unprotected agent stops (at once when it is between turns), so the
//! thread's next send needs a launch, which is refused.

use std::sync::{Arc, Mutex, PoisonError};

use clauseo_protocol::{
    AgentKind, CommandOutcome, DetachReason, DomainEvent, ErrorCode, HostCommand,
    NotProtectedCause, Protection, StopReason, StreamId, ThreadId,
};
use clauseo_sandbox::{Blocked, NoSandbox, SetupState};

use crate::codex::{Recording, turn_failed};
use crate::limits_host::{CLAUDE, Reads};
use crate::observe::Events;
use crate::permissions::{KINDS_ANSWERS, answer_from, until_first_request};
use crate::protection::dir;
use crate::protection_rig::{Plan, c, rig, through, view};
use crate::support::{Dir, FAKE, Running, Scenario};

/// One launch's environment, as `Recording` keeps it.
type Env = std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>;

/// The settings that let an agent run without the sandbox, in an auto mode too, with `agents`.
fn lenient(dir: &Dir, agents: &[(&str, Scenario)]) {
    dir.set_settings(
        agents,
        serde_json::json!({"sandbox": {"required": false, "allow_unprotected_auto": true}}),
    );
}

/// A failed set-up and a failed probe: both `SetUpFailed`, both refused with `sandbox.blocked`
/// though both settings would let an unprotected agent run.
#[test]
fn a_broken_set_up_refuses_every_launch_whatever_the_settings() {
    let dir = dir("protection_broken_create");
    lenient(&dir, &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let cases = [
        (
            c(NoSandbox::SetUpFailed(
                "the record is unreadable".to_owned(),
            )),
            SetupState::Failed("the record is unreadable".to_owned()),
        ),
        (
            c(NoSandbox::ProbeFailed("no build number".to_owned())),
            SetupState::Ready,
        ),
    ];
    for (run, (detection, state)) in (0u16..).zip(cases) {
        let (host, _rig) =
            crate::protection::start(&dir, run, Plan::new(vec![detection], vec![state]));
        let answer = view(&host);
        let cause = NotProtectedCause::SetUpFailed;
        assert_eq!(
            answer.computer,
            Protection::NotProtected { cause },
            "run {run}"
        );
        assert!(!answer.required && answer.unprotected_auto_allowed);
        let matter = host.matters()[0].matter;
        let command = HostCommand::CreateThread {
            matter,
            agent: clauseo_protocol::AgentId::new(FAKE).unwrap(),
        };
        drop(host.refused(command, ErrorCode::SandboxBlocked));
        host.shutdown();
    }
}

/// A blocked sandbox starts no limits reader; a set-up not done yet, with `sandbox.required`
/// off, does.
#[test]
fn a_blocked_sandbox_starts_no_limits_reader() {
    let dir = Dir::new("protection_broken_limits", &[]);
    lenient(&dir, &[(CLAUDE, Scenario::Bundled("prompt-stream"))]);
    let cases = [
        (
            c(NoSandbox::SetUpBlocked(Blocked::Policy)),
            SetupState::Blocked(Blocked::Policy),
            0,
        ),
        (c(NoSandbox::NeedsSetUp), SetupState::Needed, 1),
    ];
    for (run, (detection, state, reads_wanted)) in (0u16..).zip(cases) {
        let reads = Reads::new();
        let reader = Arc::new(reads.clone());
        let sandbox = rig(Plan::new(vec![detection], vec![state])).sandbox;
        let host = dir.start_with(run, move |config| {
            config.with_sandbox(sandbox).with_limits_reader(reader)
        });
        if reads_wanted > 0 {
            reads.until_count(AgentKind::Claude, reads_wanted);
        }
        host.settle();
        assert_eq!(reads.count(AgentKind::Claude), reads_wanted, "run {run}");
        host.shutdown();
    }
}

/// A host of run `run` on `dir` whose sandbox is first found needing its set-up, then, at
/// the first `RetryProtection` (a check: the set-up reads as done), blocked; with the launches
/// it makes.
fn turning_blocked(dir: &Dir, run: u16) -> (Running, Arc<Mutex<Vec<Env>>>) {
    let plan = Plan::new(
        vec![
            c(NoSandbox::NeedsSetUp),
            c(NoSandbox::SetUpBlocked(Blocked::Policy)),
        ],
        vec![SetupState::Ready],
    );
    let recording = Arc::new(Recording::new(dir));
    let launches = Arc::clone(&recording.envs);
    let sandbox = through(rig(plan).sandbox, Arc::clone(&recording) as _);
    let host = dir.start_launching(run, recording, move |config| config.with_sandbox(sandbox));
    (host, launches)
}

/// A matter with one thread of `FAKE` on `dir`, made by run 0 with no sandbox.
fn one_thread(dir: &Dir) -> ThreadId {
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[("brief.txt", "x\n")]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    thread
}

/// `RetryProtection`'s check finds the block; it answers once its detection is in.
fn check_finds_the_block(host: &Running) {
    assert_eq!(
        host.ok(HostCommand::RetryProtection {}),
        CommandOutcome::Done
    );
    let cause = NotProtectedCause::Blocked {
        reason: "policy".to_owned(),
    };
    assert_eq!(view(host).computer, Protection::NotProtected { cause });
}

/// `thread`'s next send is refused with `sandbox.blocked`, and no agent starts for it.
fn next_send_refused(
    host: &Running,
    events: &mut Events,
    thread: ThreadId,
    launches: &Mutex<Vec<Env>>,
) {
    let (turn, _) = host.prompt(thread, "Look at the matter again.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::Failed, "{}", events.dump());
    let code = turn_failed(events, turn).map(|failed| failed.code);
    assert_eq!(code, Some(ErrorCode::SandboxBlocked), "{}", events.dump());
    host.settle();
    let launched = launches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .len();
    assert_eq!(launched, 1, "an agent started for the refused send");
}

/// The orchestrator's ruling: a turn running unprotected, as the settings let it, when the
/// computer's cause turns `Blocked` finishes on its own (stopping it would lose the lawyer's
/// work and protect nothing); its agent stops then, so the thread's next send needs a launch,
/// which is refused.
#[test]
fn a_running_turn_finishes_when_the_sandbox_turns_blocked_and_its_next_send_is_refused() {
    let dir = Dir::new("protection_broken_running", &[]);
    lenient(&dir, &[(FAKE, Scenario::Bundled("permission-kinds"))]);
    let thread = one_thread(&dir);
    let (host, launches) = turning_blocked(&dir, 1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Edit the brief.");
    until_first_request(&mut events);
    // The turn waits for the lawyer's answer as a check finds the sandbox blocked.
    check_finds_the_block(&host);
    answer_from(&host, &mut events, thread, 0, &KINDS_ANSWERS);
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert_eq!(turn_failed(&events, turn), None);
    let launched = launches.lock().unwrap().len();
    assert_eq!(launched, 1);
    next_send_refused(&host, &mut events, thread, &launches);
    host.shutdown();
}

/// A live unprotected session between turns stops as soon as the block is found; the next
/// send is refused.
#[test]
fn an_idle_unprotected_session_stops_when_the_sandbox_turns_blocked() {
    let dir = Dir::new("protection_broken_idle", &[]);
    lenient(&dir, &[(FAKE, Scenario::Bundled("prompt-stream"))]);
    let thread = one_thread(&dir);
    let (host, launches) = turning_blocked(&dir, 1);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    let detached = |events: &Events| {
        events.events().any(|event| {
            matches!(event, DomainEvent::AgentSessionDetached(d)
                if d.reason == DetachReason::IdleStopped)
        })
    };
    assert!(!detached(&events), "the session stopped before the block");
    check_finds_the_block(&host);
    events.until("the idle session's stop", detached);
    next_send_refused(&host, &mut events, thread, &launches);
    host.shutdown();
}
