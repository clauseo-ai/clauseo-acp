//! The launcher seam (decision record 0051, R17 PR-B2): with `Sandbox::with_launchers` every
//! agent launch asks the seam for its launcher off the actor, with the launch's policy, and
//! spawns through it; the names it set aside give one notice; its `after_session` follows the
//! agent's exit. Without launchers the launch is the host launcher's, as before. The sessions'
//! lifecycle across threads and relaunches is in `sandbox_sessions.rs`.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use clauseo_protocol::{DomainEvent, ErrorCode, StopReason, StreamId};

use crate::sandbox_rig::{
    Answer, Fake, LAUNCH_WAIT, attached, canonical, dir, failed, notices, run_entries,
    shutdown_while, start, until_no_run_folder, until_notices,
};
use crate::support::{FAKE, matter_with_thread};

/// The session folder's name of a session's `tmp` folder.
fn session_name(tmp: &Path) -> String {
    tmp.parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn with_launchers_a_launch_asks_once_with_its_policy_and_spawns_through_the_seam() {
    let dir = dir("sandbox_launch_seam", "prompt-stream");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );

    let calls = fake.calls();
    assert_eq!(calls.len(), 1, "one launcher per launch");
    let call = &calls[0];
    let policy = &call.policy;
    assert_eq!(call.matter, matter);
    assert_eq!((call.session.matter, call.session.thread), (matter, thread));
    assert_eq!(call.running, [], "no other session runs in the matter");
    assert_eq!(policy.matter(), canonical(&folder));
    // TMPDIR is the session's `run/<session>/tmp`, made with the run folder; the run folder
    // itself is not granted.
    let tmp = policy.session_tmpdir().expect("the session's tmp");
    assert_eq!(tmp.file_name().unwrap(), "tmp");
    assert!(tmp.is_dir(), "{tmp:?}");
    assert_eq!(
        session_name(tmp),
        call.session.id(),
        "the folder is the session's"
    );
    let session = tmp.parent().unwrap();
    assert_eq!(
        session.parent().unwrap(),
        canonical(&dir.data_dir().join("run"))
    );
    assert_ne!(policy.state(), session);
    assert!(policy.state().starts_with(session), "{policy:?}");
    // The agent's install folder, read and run.
    let install = canonical(clauseo_fake_agent::bin_path().unwrap().parent().unwrap());
    assert_eq!(policy.runtime(), [install]);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 1);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    // No set-aside, no notice.
    assert_eq!(
        events.count(|event| matches!(event, DomainEvent::Notice(_))),
        0
    );
    host.shutdown();
    assert_eq!(fake.calls().len(), 1);
    // The agent stopped with the host: its after-session ran once before shutdown answered,
    // with the launch's own session and policy.
    let after = fake.after();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].matter, matter);
    assert_eq!(after[0].session, call.session);
    assert_eq!(after[0].policy, *policy);
}

/// A linked command (`/opt/homebrew/bin/claude`) grants its real install folder, never the
/// link's folder.
#[cfg(unix)]
#[test]
fn a_linked_command_grants_the_real_install_folder_not_the_links() {
    let dir = dir("sandbox_launch_link", "prompt-stream");
    let real = clauseo_fake_agent::bin_path().unwrap();
    let links = dir.home().join("bin");
    crate::write::create_dir_all(&links);
    let link = links.join("fake_agent");
    crate::write::symlink(&real, &link);
    let scenario = crate::support::Scenario::Bundled("prompt-stream").arg();
    let settings = serde_json::json!({ "agents": { FAKE: {
        "command": link.display().to_string(),
        "args": ["--scenario", scenario],
    }}});
    crate::write::write(
        &dir.data().join("settings.json"),
        settings.to_string().as_bytes(),
    );
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    let calls = fake.calls();
    let runtime = calls[0].policy.runtime();
    assert_eq!(runtime, [canonical(real.parent().unwrap())]);
    assert!(!runtime.contains(&canonical(&links)), "{runtime:?}");
    assert_eq!(seam_launched.load(Ordering::SeqCst), 1);
    host.shutdown();
}

#[test]
fn without_launchers_nothing_is_asked_and_the_host_launcher_spawns() {
    let dir = dir("sandbox_launch_none", "prompt-stream");
    let (host, host_launched) = start(&dir, None);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(host_launched.load(Ordering::SeqCst), 1);
    host.shutdown();
}

#[test]
fn a_launcher_error_fails_the_launch_naming_its_cause_and_leaves_nothing() {
    let dir = dir("sandbox_launch_error", "prompt-stream");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Fail);
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    let (code, detail) = failed(&events, turn).expect("TurnFailed");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    let detail = detail.unwrap_or_default();
    assert!(detail.contains("sandbox"), "{detail}");
    let matter_path = canonical(&folder);
    for path in [matter_path.as_path(), folder.as_path(), dir.data_dir()] {
        assert!(!detail.contains(&*path.to_string_lossy()), "{detail}");
    }
    host.settle();
    events.drain_ready();
    assert_eq!(attached(&events), 0, "{}", events.dump());
    assert!(run_entries(&dir).is_empty(), "{:?}", run_entries(&dir));
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    host.shutdown();
    assert_eq!(
        fake.after_matters(),
        [],
        "no agent spawned: no after-session"
    );
}

#[test]
fn a_launcher_held_past_the_bound_fails_the_launch_while_the_host_answers() {
    let dir = dir("sandbox_launch_held", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(vec![".git"]));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    // The launch checks' wait, then the seam's.
    host.wait_armed(LAUNCH_WAIT);
    host.wait_armed(LAUNCH_WAIT);
    // The actor answers while the seam holds.
    assert_eq!(host.threads(matter).len(), 1);
    host.clock.advance(LAUNCH_WAIT);
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    let (code, detail) = failed(&events, turn).expect("TurnFailed");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert!(detail.unwrap_or_default().contains("sandbox"));
    // The late answer is taken back on its thread: nothing spawns, its run folder goes; the
    // name it moved still gets its notice.
    drop(release);
    until_no_run_folder(&dir);
    let notices = until_notices(&host, thread, 1);
    assert!(notices[0].contains(".git"), "{notices:?}");
    host.settle();
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    host.shutdown();
    assert_eq!(fake.after_matters(), []);
}

#[test]
fn two_names_set_aside_give_one_notice_naming_both_relative() {
    let dir = dir("sandbox_launch_set_aside", "prompt-stream");
    // One behind Windows' `\\?\` prefix, and one outside the matter, which is never shown.
    let names = vec![".git", "verbatim:sub/AGENTS.md", "/elsewhere/CLAUDE.md"];
    let (fake, _) = Fake::new(&dir, Answer::Launch(names));
    let (host, _) = start(&dir, Some(fake));
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    let notices = notices(&events);
    assert_eq!(notices.len(), 1, "{notices:?}");
    let text = &notices[0];
    assert!(
        text.contains(".git, sub/AGENTS.md.") && text.contains("2 protected items"),
        "{text}"
    );
    assert!(
        !text.contains("CLAUDE.md") && !text.contains("elsewhere"),
        "{text}"
    );
    for path in [
        canonical(&folder),
        folder.clone(),
        dir.data_dir().to_owned(),
    ] {
        assert!(!text.contains(&*path.to_string_lossy()), "{text}");
    }
    host.shutdown();
}

#[test]
fn after_session_is_called_once_when_the_agent_exits() {
    let dir = dir("sandbox_launch_after", "exit-mid-turn");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Start the answer.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    assert_eq!(
        failed(&events, turn).map(|(code, _)| code),
        Some(ErrorCode::AgentExited)
    );
    fake.until_after(1);
    until_no_run_folder(&dir);
    host.settle();
    // It moved nothing: no notice.
    events.drain_ready();
    assert_eq!(notices(&events), [] as [String; 0], "{}", events.dump());
    host.shutdown();
    assert_eq!(fake.after_matters(), [matter]);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 1);
}

/// A held seam keeps shutdown waiting no longer than a launch's checks would (the launch's
/// wait); its answer during shutdown is taken back, so nothing spawns and the run folder goes.
#[test]
fn shutdown_during_a_held_launcher_ends_cleanly_and_the_late_answer_spawns_nothing() {
    let dir = dir("sandbox_launch_shutdown", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(vec![".git"]));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    shutdown_while(host, async move {
        // The shutdown is under way by then; the seam answers into it.
        async_io::Timer::after(Duration::from_millis(50)).await;
        drop(release);
    });
    until_no_run_folder(&dir);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(fake.after_matters(), []);
}

/// A seam that never answers holds shutdown only until the launch's wait passes on the host
/// clock; the answer that comes after the host is gone spawns nothing.
#[test]
fn shutdown_past_a_seam_that_never_answers_ends_at_the_launch_wait() {
    let dir = dir("sandbox_launch_shutdown_wait", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    host.wait_armed(LAUNCH_WAIT);
    host.wait_armed(LAUNCH_WAIT);
    let clock = host.clock.clone();
    shutdown_while(host, async move {
        async_io::Timer::after(Duration::from_millis(50)).await;
        clock.advance(LAUNCH_WAIT);
    });
    drop(release);
    until_no_run_folder(&dir);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    assert_eq!(fake.after_matters(), []);
}
