//! A Codex thread last in Read-only, reopened (R17 PR-D2c round 4). Unjailed (tier C) it starts
//! in the ask mode, `workspace-write`, never the full-access preset, with no notice. Jailed it
//! runs the preset: in the Read-only policy where the jail enforces it; on Windows, until PR6,
//! in the normal jailed policy, and the lawyer is told once (`NoticeKind::ReadOnlyUnavailable`).
//!
//! Run 0 saves Read-only on every system: an unjailed Codex that moves itself there is logged as
//! it came (`forced_mode::agent_moved`).

use std::sync::Arc;

use clauseo_protocol::{DomainEvent, NoticeKind, Seq, StreamId, ThreadId};

use crate::codex::{CODEX, Recording, codex_dir, run_turn};
use crate::codex_jailed::modes;
use crate::observe::Events;
use crate::sandbox_rig::{Answer, Fake, start_run};
use crate::support::{Dir, Scenario, matter_with_thread};

/// Every host notice's kind, in order.
fn notice_kinds(events: &Events) -> Vec<Option<NoticeKind>> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.kind.clone()),
            _ => None,
        })
        .collect()
}

/// Run 0, unjailed: the thread's saved mode becomes `read-only`. Its thread and stream head.
fn saved_read_only(dir: &Dir) -> (ThreadId, u64) {
    dir.set_agents(&[(CODEX, Scenario::Local("codex-saves-read-only"))]);
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(dir, &host, "m", &[("a.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Read only now.",
        "{}",
        events.dump()
    );
    assert_eq!(modes(&events), ["workspace-write", "read-only"]);
    let head = events.head();
    host.shutdown();
    (thread, head)
}

#[test]
fn an_unjailed_codex_last_in_read_only_starts_in_the_ask_mode() {
    let dir = codex_dir("codex_saved_ro_unjailed", "codex-handshake");
    let (thread, head) = saved_read_only(&dir);

    dir.set_agents(&[(CODEX, Scenario::Local("codex-resumes-unjailed"))]);
    let launcher = Recording::new(&dir);
    let envs = Arc::clone(&launcher.envs);
    let host = dir.start_launching(1, Arc::new(launcher), |config| config);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Asking as before.",
        "{}",
        events.dump()
    );
    assert_eq!(modes(&events), ["workspace-write"]);
    assert_eq!(notice_kinds(&events), [], "{}", events.dump());
    host.shutdown();
    let envs = envs.lock().unwrap();
    let [env] = envs.as_slice() else {
        panic!("one launch, not {}", envs.len());
    };
    let mode = env.get(&std::ffi::OsString::from("INITIAL_AGENT_MODE"));
    assert_eq!(mode.and_then(|mode| mode.to_str()), Some("workspace-write"));
}

#[test]
fn a_jailed_codex_last_in_read_only_keeps_it_or_is_told_once() {
    let dir = codex_dir("codex_saved_ro_jailed", "codex-handshake");
    let (thread, head) = saved_read_only(&dir);

    dir.set_agents(&[(CODEX, Scenario::Local("codex-jailed-saved-full"))]);
    let (fake, _) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _, _) = start_run(&dir, 1, Some(Arc::clone(&fake)), |config| config);
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let turn = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(
        events.turn_text(turn),
        "Still on its own.",
        "{}",
        events.dump()
    );
    let calls = fake.calls();
    if cfg!(windows) {
        // TODO(PR6): Read-only is not enforced here yet; the normal jailed policy, told once.
        assert!(!calls[0].policy.matter_read_only());
        assert_eq!(modes(&events), ["agent-full-access"]);
        assert_eq!(
            notice_kinds(&events),
            [Some(NoticeKind::ReadOnlyUnavailable)],
            "{}",
            events.dump()
        );
    } else {
        assert!(calls[0].policy.matter_read_only());
        assert_eq!(modes(&events), ["read-only"]);
        assert_eq!(notice_kinds(&events), [], "{}", events.dump());
    }
    host.shutdown();
}
