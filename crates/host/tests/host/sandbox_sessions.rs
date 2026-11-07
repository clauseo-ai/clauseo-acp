//! The sandbox seam across a matter's sessions (R17 PR-B2): each launch is told the matter's
//! other sessions still running, each spawned agent gets one `after_session` with its launch's
//! policy (across an exit and a relaunch too), what that call moved gives one notice, and a
//! panicking seam leaves no run folder.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use clauseo_protocol::{ErrorCode, StopReason, StreamId, ThreadId, TurnId};

use crate::observe::Events;
use crate::sandbox_rig::{
    Answer, Fake, failed, notices, run_entries, start, until_no_run_folder, until_notices,
};
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread};

/// An agent whose first prompt ends the turn and whose second makes it exit.
const EXITS_SECOND: &str = "exits-second";

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (FAKE, Scenario::Bundled("prompt-stream")),
            (EXITS_SECOND, Scenario::Local("sandbox-exit-second")),
        ],
    )
}

/// Sends `prompt` to `thread` and waits for its end.
fn turn(host: &Running, thread: ThreadId, prompt: &str) -> (Events, TurnId, StopReason) {
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, prompt);
    let stop = events.until_turn_ended(turn).stop;
    (events, turn, stop)
}

#[test]
fn a_launch_is_told_the_matters_other_running_sessions_and_not_ended_ones() {
    let dir = dir("sandbox_sessions_running");
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, first, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], EXITS_SECOND);
    let second = host.create_thread(matter, FAKE);
    let third = host.create_thread(matter, FAKE);
    // A thread of another matter runs too; it is never named.
    let (_, elsewhere, _) = matter_with_thread(&dir, &host, "n", &[("a.txt", "y\n")], FAKE);

    assert_eq!(turn(&host, elsewhere, "Read it.").2, StopReason::EndTurn);
    assert_eq!(turn(&host, first, "Read it.").2, StopReason::EndTurn);
    assert_eq!(turn(&host, second, "Read it.").2, StopReason::EndTurn);
    let calls = fake.calls();
    let first_key = calls[1].session;
    assert_eq!(first_key.thread, first);
    assert_eq!(calls[2].session.thread, second);
    assert_eq!(
        calls[2].running,
        [first_key],
        "the second launch is told the first"
    );

    // The first session ends (its agent exits); the next launch is not told it.
    let (events, ended, stop) = turn(&host, first, "Go on.");
    assert_eq!(stop, StopReason::Failed, "{}", events.dump());
    assert_eq!(
        failed(&events, ended).map(|f| f.0),
        Some(ErrorCode::AgentExited)
    );
    fake.until_after(1);
    assert_eq!(fake.after()[0].session, first_key);
    assert_eq!(turn(&host, third, "Read it.").2, StopReason::EndTurn);
    let calls = fake.calls();
    assert_eq!(calls[3].session.thread, third);
    assert_eq!(
        calls[3].running,
        [calls[2].session],
        "only the second still runs"
    );
    host.shutdown();
}

#[test]
fn after_session_is_called_once_per_spawned_agent_across_an_exit_and_a_relaunch() {
    let dir = dir("sandbox_sessions_relaunch");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], EXITS_SECOND);
    assert_eq!(turn(&host, thread, "Read it.").2, StopReason::EndTurn);
    assert_eq!(turn(&host, thread, "Go on.").2, StopReason::Failed);
    fake.until_after(1);
    // The next prompt relaunches the agent: a second launch, a second session.
    assert_eq!(turn(&host, thread, "Read it again.").2, StopReason::EndTurn);
    host.settle();
    assert_eq!(fake.after().len(), 1, "the relaunched agent still runs");
    host.shutdown();
    let (calls, after) = (fake.calls(), fake.after());
    assert_eq!(calls.len(), 2);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 2);
    assert_eq!(after.len(), 2, "one after-session per spawned agent");
    assert_ne!(calls[0].session, calls[1].session);
    for (call, end) in calls.iter().zip(&after) {
        assert_eq!(end.session, call.session);
        assert_eq!(end.policy, call.policy, "the launch's own policy");
    }
}

#[test]
fn what_after_session_moved_gives_one_notice_relative_to_the_matter() {
    let dir = dir("sandbox_sessions_after_notice");
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    fake.move_after(&["verbatim:.git", "sub/CLAUDE.md", "/elsewhere/AGENTS.md"]);
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], EXITS_SECOND);
    let (events, _, _) = turn(&host, thread, "Read it.");
    assert_eq!(
        notices(&events),
        [] as [String; 0],
        "nothing moved before the launch"
    );
    assert_eq!(turn(&host, thread, "Go on.").2, StopReason::Failed);
    fake.until_after(1);
    let notices = until_notices(&host, thread, 1);
    assert_eq!(notices.len(), 1, "{notices:?}");
    let text = &notices[0];
    assert!(
        text.starts_with("After the agent's session ended"),
        "{text}"
    );
    assert!(text.contains(".git, sub/CLAUDE.md."), "{text}");
    assert!(
        !text.contains("AGENTS.md") && !text.contains("elsewhere"),
        "{text}"
    );
    assert!(!text.contains(&*folder.to_string_lossy()), "{text}");
    host.shutdown();
}

#[test]
fn a_panicking_seam_fails_the_launch_and_leaves_no_run_folder() {
    let dir = dir("sandbox_sessions_panic");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Panic);
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let (events, ended, stop) = turn(&host, thread, "Read it.");
    assert_eq!(stop, StopReason::Failed, "{}", events.dump());
    assert_eq!(
        failed(&events, ended).map(|f| f.0),
        Some(ErrorCode::AgentLaunchFailed)
    );
    // The prep's guard took the run folder back as the seam's thread unwound.
    until_no_run_folder(&dir);
    assert_eq!(run_entries(&dir), [] as [std::path::PathBuf; 0]);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    host.shutdown();
    assert_eq!(fake.after_matters(), []);
}
