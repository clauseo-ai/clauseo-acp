//! R17 PR-D1: Claude's state folders and the matters' notes leave `<data>/agent-state` at the
//! host's start (`agent/state_folder/layout.rs`), before anything launches; Codex's state stays.
//! A thread from the older layout resumes with its Claude state and its notes found.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{MatterId, Seq, StopReason, StreamId};

use crate::codex::{CODEX, run_turn};
use crate::profile_rig::{CLAUDE, Wired, notes, state_folder};
use crate::state_folders::{turn_on, two_agents, var};
use crate::support::{Dir, Scenario, matter_with_thread};

/// A transcript Claude keeps across launches, in its state folder.
const TRANSCRIPT: &str = "projects/-matter/s-old.jsonl";

/// The matter's folder in `agent-state`, where Claude's state and the notes were before.
fn old_matter(dir: &Dir, matter: MatterId) -> PathBuf {
    dir.data().join("agent-state").join(matter.to_string())
}

/// Whether the matter is in the new layout, with the transcript and the note, and nothing left
/// at the old places; Codex's state folder where it always was.
fn in_new_layout(dir: &Dir, matter: MatterId) -> bool {
    let read = |path: &Path| std::fs::read(path).unwrap_or_default();
    let old = old_matter(dir, matter);
    read(&state_folder(dir, matter, CLAUDE).join(TRANSCRIPT)) == b"{}\n"
        && read(&notes(dir, matter).join("brief.md")) == b"note"
        && !old.join(CLAUDE).exists()
        && !old.join("notes").exists()
        && old.join(CODEX).is_dir()
}

/// An older app's layout (Claude's state folder and the notes in `agent-state/<matter>`) moves at
/// the next start, before any launch; the Claude thread resumes there with its transcript and
/// the notes, and a third start moves nothing.
#[test]
fn an_old_layout_moves_at_the_start_and_its_claude_thread_resumes() {
    let dir = two_agents("state_layout_moves");
    let wired = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&wired), |config| config);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "a", &[("brief.txt", "x\n")], CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    run_turn(&host, &mut events, thread);
    turn_on(&host, matter, CODEX);
    let head = events.head();
    host.shutdown();

    // Back to the layout before R17 PR-D1, as an older app left it.
    let claude = state_folder(&dir, matter, CLAUDE);
    let note = notes(&dir, matter);
    crate::write::create_dir_all(claude.join(TRANSCRIPT).parent().unwrap());
    crate::write::write(&claude.join(TRANSCRIPT), b"{}\n");
    crate::write::create_dir_all(&note);
    crate::write::write(&note.join("brief.md"), b"note");
    let old = old_matter(&dir, matter);
    crate::write::rename(&claude, &old.join(CLAUDE));
    crate::write::rename(&note, &old.join("notes"));
    assert!(!in_new_layout(&dir, matter));
    dir.set_agents(&[
        (CLAUDE, Scenario::Local("claude-layout-load")),
        (CODEX, Scenario::Bundled("codex-handshake")),
    ]);

    let host = dir.start_launching(1, Arc::<Wired>::clone(&wired), |config| config);
    assert!(
        in_new_layout(&dir, matter),
        "moved at the start, before any launch"
    );
    let mut events = host.events_after(StreamId::Thread(thread), Seq::new(head).ok());
    events.until_caught_up();
    let second = run_turn(&host, &mut events, thread);
    events.assert_no_mismatch();
    assert_eq!(events.turn_ended(second).unwrap().stop, StopReason::EndTurn);
    assert_eq!(
        events.turn_text(second),
        "Resumed.",
        "session/load found the session"
    );
    host.shutdown();
    let seen = wired.seen();
    let resumed = seen.last().expect("the resumed launch");
    assert_eq!(
        var(resumed, "CLAUDE_CONFIG_DIR"),
        Some(claude.display().to_string())
    );
    assert!(in_new_layout(&dir, matter), "the launch's clean kept both");

    let host = dir.start_launching(2, Arc::<Wired>::clone(&wired), |config| config);
    host.shutdown();
    assert!(in_new_layout(&dir, matter), "a second start moves nothing");
}
