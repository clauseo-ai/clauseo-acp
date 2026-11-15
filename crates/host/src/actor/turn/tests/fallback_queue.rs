//! A file the browser guard saved in Clauseo's own downloads folder for the matter in its last
//! sweep, after its last tool result (the guard killed, its reaper's sweep delivering it): no
//! agent line tells it, so the host reads the guard's queue in that folder when the session ends,
//! or when it starts again after a crash, and shows the lawyer the notice without the session
//! resuming (`launch_checks/removal/saved.rs`).

use clauseo_protocol::{DomainEvent, NoticeKind, ThreadId};

use super::agents::{send_prompt, until_settled};
use super::rig::Rig;

/// The agent answers the prompt, then exits while idle: the session ends.
const EXIT_AFTER_ANSWER: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"exit":{"code":0}}
"#;

/// Writes what a killed guard's reaper leaves: `judgment.pdf` in the matter's fallback folder
/// and the queue naming it, for `thread`'s session.
fn reaper_delivered(rig: &Rig, thread: ThreadId) {
    let data = rig.actor.effective.data_dir().to_owned();
    let folder = crate::agent::loads::browser::matter_fallback_downloads(&data, rig.matter());
    crate::write::create_dir_all(&folder).unwrap();
    let file = folder.join("judgment.pdf");
    crate::write::write_file(&file, b"%PDF").unwrap();
    let note = format!(
        "Saved judgment.pdf outside the matter, in Clauseo's own folder for this matter: the \
         matter's drive cannot keep the \"downloaded from the internet\" mark, so the file is \
         not in downloads/. Your user sees where it is in the thread. Its path: {}",
        file.display()
    );
    let queue = serde_json::json!({"session": format!("{thread}-1"), "notes": [note]});
    crate::write::write_file(
        &folder.join(".clauseo-untold-0123456789abcdef"),
        queue.to_string().as_bytes(),
    )
    .unwrap();
}

/// The `DownloadOutsideMatter` notices in `thread`'s log, by name.
fn download_notices(rig: &mut Rig, thread: ThreadId) -> Vec<String> {
    rig.log(thread)
        .into_iter()
        .filter_map(|event| {
            let DomainEvent::Notice(notice) = event else {
                return None;
            };
            let Some(NoticeKind::DownloadOutsideMatter { name }) = notice.kind else {
                return None;
            };
            Some(name)
        })
        .collect()
}

/// The launch makes the fallback folder before the fences list the matter's state, so both deny
/// it from the first session on (a folder the guard made mid-session would be in neither).
#[test]
fn the_launch_makes_the_fallback_folder_and_the_fences_deny_it() {
    let mut rig = Rig::start("turn-fallback-folder-fenced", EXIT_AFTER_ANSWER);
    let thread = rig.thread();
    let data = rig.actor.effective.data_dir().to_owned();
    let folder = crate::agent::loads::browser::matter_fallback_downloads(&data, rig.matter());
    assert!(!folder.exists());
    let _turn = send_prompt(&mut rig, 10, thread);
    rig.run_until("the launch", |actor| {
        actor.threads[&thread].turn.run.is_some()
    });
    assert!(crate::write::is_real_dir(&folder));
    let run = rig.actor.threads[&thread].turn.run.as_ref().unwrap();
    let codex = run.codex_listed.agent_state.clone().unwrap_or_default();
    assert!(codex.contains(&folder), "{codex:?}");
    let pattern = format!("agent-state/{}/downloads/**", rig.matter());
    let claude = &run.data_deny.targets;
    assert!(claude.contains(&pattern), "{claude:?}");
    until_settled(&mut rig, thread);
    rig.finish();
}

#[test]
fn a_download_the_dead_guard_queued_is_shown_when_the_session_ends() {
    let mut rig = Rig::start("turn-fallback-queue-end", EXIT_AFTER_ANSWER);
    let thread = rig.thread();
    reaper_delivered(&rig, thread);
    let _turn = send_prompt(&mut rig, 10, thread);
    until_settled(&mut rig, thread);
    rig.run_until("the run folder's removal and the notice", |actor| {
        actor.work.run_removals == 0 && actor.work.loads == 0 && actor.work.commits == 0
    });
    assert_eq!(download_notices(&mut rig, thread), ["judgment.pdf"]);
    rig.finish();
}

#[test]
fn a_download_queued_before_a_crash_is_shown_at_the_next_start_once() {
    let mut rig = Rig::start("turn-fallback-queue-start", "");
    let thread = rig.thread();
    reaper_delivered(&rig, thread);
    let mut rig = rig.restart(1);
    rig.run_until("the notice", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    assert_eq!(download_notices(&mut rig, thread), ["judgment.pdf"]);
    // The queue is gone: the next start shows nothing more.
    let mut rig = rig.restart(2);
    rig.run_until("the start", |actor| {
        actor.work.loads == 0 && actor.work.commits == 0
    });
    assert_eq!(download_notices(&mut rig, thread), ["judgment.pdf"]);
    rig.finish();
}
