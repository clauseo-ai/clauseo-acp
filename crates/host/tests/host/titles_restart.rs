//! The thread's title across an app restart (D-070). The name settles after the first answered
//! turn; on load the host folds in that the answered turn already ended, so a title the agent
//! replays when its session is loaded again (codex-acp replays its title on `session/load`) is
//! ignored, even within `TITLE_GRACE` of that answer. A first turn with no answer text settles
//! nothing, so after a restart the replayed title is taken. Scenarios: `title-seed` (one answered
//! turn) then `title-load`; `title-seed-error` (one failed turn, no text) then
//! `title-load-unanswered`. The cases with no restart: `titles.rs`.

use clauseo_protocol::{RenameSource, StreamId};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};
use crate::titles::{AGENT_TITLE, FIRST_WORDS, rename, renames_in, stored_sources, titles, turn};

/// One answered first turn, a restart on the same data folder, then the thread opened and a
/// prompt: `session/load` replays the agent's title before its response. The host is restarted
/// well within `TITLE_GRACE` of the answer, so only the loaded history settles the name: no
/// rename by the agent, and the first question's title stays in the row, header and store.
#[test]
fn restarted_host_ignores_a_replayed_title_after_an_answered_first_turn() {
    let dir = Dir::new(
        "title_restart_answered",
        &[(FAKE, Scenario::Local("title-seed"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    rename(&host, thread, FIRST_WORDS, true);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, FIRST_WORDS);
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("title-load"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "And clause 10?");
    assert_eq!(
        renames_in(&events),
        [(FIRST_WORDS.to_owned(), RenameSource::Placeholder)],
        "no ThreadRenamed by Agent: {}",
        events.dump()
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(FIRST_WORDS), "sidebar row");
    assert_eq!(header.as_deref(), Some(FIRST_WORDS), "header");
    host.shutdown();
    assert_eq!(
        stored_sources(&dir, matter),
        [Some(RenameSource::Placeholder)]
    );
}

/// A first turn that failed before any answer text, a restart, then the thread opened and a
/// prompt: the title `session/load` replays is taken, since nothing settled the name.
#[test]
fn restarted_host_takes_a_replayed_title_after_an_unanswered_first_turn() {
    let dir = Dir::new(
        "title_restart_unanswered",
        &[(FAKE, Scenario::Local("title-seed-error"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    rename(&host, thread, FIRST_WORDS, true);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(thread, FIRST_WORDS);
    events.until_turn_ended(first);
    host.settle();
    events.drain_ready();
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("title-load-unanswered"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "And clause 10?");
    assert_eq!(
        renames_in(&events),
        [
            (FIRST_WORDS.to_owned(), RenameSource::Placeholder),
            (AGENT_TITLE.to_owned(), RenameSource::Agent),
        ],
        "{}",
        events.dump()
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(AGENT_TITLE), "sidebar row");
    assert_eq!(header.as_deref(), Some(AGENT_TITLE), "header");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Agent)]);
}
