//! The thread's title, at the interface. Rule 1: a title the lawyer typed always wins. Rule 2:
//! the agent's own title (`session_info_update`) replaces Clauseo's first-words placeholder, and
//! an earlier agent title. Rule 3: the placeholder shows only until the agent's title arrives,
//! so a placeholder never replaces a title. The row keeps who set its title (`title_source`),
//! which the tests read back from the store after a host stops.
//!
//! The agent's side comes from scenarios: `title-after-rename` (a title on the second turn),
//! `title-seed` and `title-load` (a title replayed by `session/load`),
//! `title-load-replay-in-prompt` (a title replayed by the first prompt after a reload, as
//! claude-agent-acp does), `title-codex-double` (codex-acp's prompt title, then its AI title)
//! and `title-turns` (three turns with titles for the agent-title sequence).

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, DomainEvent, HostCommand, MatterId, RenameSource, StopReason, StreamId,
    ThreadId, Title,
};
use clauseo_store::{Store, StoreOptions};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The agent's title in `title-after-rename`, `title-load` and `title-load-replay-in-prompt`.
const AGENT_TITLE: &str = "Indemnity cap review";

/// The room's first-words title: the first words of the first prompt.
const FIRST_WORDS: &str = "Review the indemnity cap in clause 9";

/// The lawyer's hand-typed title.
const TYPED: &str = "Acme MSA";

/// A rename of `thread` to `title`: the app's first-words placeholder, or a title the lawyer
/// typed. The command must be answered `Done` either way.
fn rename(host: &Running, thread: ThreadId, title: &str, placeholder: bool) {
    let command = HostCommand::RenameThread {
        thread,
        title: Title::new(title).unwrap(),
        placeholder,
    };
    assert_eq!(host.ok(command), CommandOutcome::Done);
}

/// The title `ListThreads` gives (the sidebar's row) and the last `ThreadRenamed` (the header).
fn titles(host: &Running, matter: MatterId, thread: ThreadId) -> (Option<String>, Option<String>) {
    let row = host
        .threads(matter)
        .into_iter()
        .find(|summary| summary.thread == thread)
        .and_then(|summary| summary.title.map(|title| title.to_string()));
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let header = renames_in(&events).last().map(|(title, _)| title.clone());
    (row, header)
}

/// The renames in `events`, in order: each title and who set it.
fn renames_in(events: &Events) -> Vec<(String, RenameSource)> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::ThreadRenamed(renamed) => Some((renamed.title.to_string(), renamed.by)),
            _ => None,
        })
        .collect()
}

/// Every rename of `thread` the host logged, from its first event.
fn renames(host: &Running, thread: ThreadId) -> Vec<(String, RenameSource)> {
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    renames_in(&events)
}

/// Opens the store in `dir`, waiting while its lock is held. A lock can outlive the host that
/// held it for a moment: another test's agent, forked before it execs, holds a copy of the
/// lock's file descriptor until then. Panics with the store's error after about ten seconds.
pub(crate) fn open_store(dir: &Path) -> Store {
    const TRIES: u32 = 2_000;
    for _ in 0..TRIES {
        match wait(
            "Store::open",
            Store::open(StoreOptions::new(dir.to_owned())),
        ) {
            Ok(store) => return store,
            Err(clauseo_store::Error::Locked) => {
                wait(
                    "a store lock",
                    async_io::Timer::after(Duration::from_millis(5)),
                );
            }
            Err(error) => panic!("Store::open: {error}"),
        }
    }
    panic!("Store::open: the store stayed locked");
}

/// The source each stored thread row holds, read after the host has stopped.
fn stored_sources(dir: &Dir, matter: MatterId) -> Vec<Option<RenameSource>> {
    let store = open_store(dir.data());
    let rows = wait("threads", store.threads(matter)).unwrap();
    rows.into_iter().map(|row| row.title_source).collect()
}

/// Two turns on `scenario`: the rename `title` (placeholder or typed) lands during the first,
/// the second's prompt carries the agent's title. The events are drained when it returns.
fn two_turns(
    test: &str,
    scenario: &'static str,
    title: &str,
    placeholder: bool,
) -> (Dir, Running, MatterId, ThreadId) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (first, _) = host.prompt(
        thread,
        "Review the indemnity cap in clause 9 and tell me if it is market",
    );
    rename(&host, thread, title, placeholder);
    assert_eq!(events.until_turn_ended(first).stop, StopReason::EndTurn);
    let (second, _) = host.prompt(thread, "And clause 10?");
    assert_eq!(events.until_turn_ended(second).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    (dir, host, matter, thread)
}

/// One turn on `scenario`, then one more prompt; the events are drained when it returns.
fn turn(host: &Running, events: &mut Events, thread: ThreadId, prompt: &str) {
    let (turn, _) = host.prompt(thread, prompt);
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
}

/// Rule 2 with the room's placeholder: the first-words title, then the agent's title. The
/// agent's title replaces it, and the row's source is the agent's.
#[test]
fn agent_title_replaces_first_words_title() {
    let (dir, host, matter, thread) = two_turns(
        "title_agent_replaces_first_words",
        "title-after-rename",
        FIRST_WORDS,
        true,
    );
    assert_eq!(
        renames(&host, thread),
        [
            (FIRST_WORDS.to_owned(), RenameSource::Placeholder),
            (AGENT_TITLE.to_owned(), RenameSource::Agent),
        ]
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(AGENT_TITLE), "sidebar row");
    assert_eq!(header.as_deref(), Some(AGENT_TITLE), "header");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Agent)]);
}

/// Rule 1: a hand-typed title, then the agent's title: the typed one stays.
#[test]
fn hand_typed_title_beats_later_agent_title() {
    let (dir, host, matter, thread) = two_turns(
        "title_typed_beats_agent",
        "title-after-rename",
        TYPED,
        false,
    );
    assert_eq!(
        renames(&host, thread),
        [(TYPED.to_owned(), RenameSource::Lawyer)],
        "no ThreadRenamed by Agent"
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(TYPED), "sidebar row");
    assert_eq!(header.as_deref(), Some(TYPED), "header");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Lawyer)]);
}

/// Rule 1 across a restart: a hand-typed title, then `session/load` replays the agent's
/// stored title before its response. The typed one stays, in the same log and after reopen.
#[test]
fn hand_typed_title_survives_load_replay() {
    let dir = Dir::new(
        "title_typed_survives_load_replay",
        &[(FAKE, Scenario::Local("title-seed"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "Review the indemnity cap");
    rename(&host, thread, TYPED, false);
    host.settle();
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("title-load"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "And clause 10?");
    assert_eq!(
        renames_in(&events),
        [(TYPED.to_owned(), RenameSource::Lawyer)],
        "{}",
        events.dump()
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(TYPED), "sidebar row");
    assert_eq!(header.as_deref(), Some(TYPED), "header");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Lawyer)]);
}

/// Claude's timing: a hand-typed title, a restart, `session/load` with no title, then the first
/// prompt carries the agent's stored title. The typed title stays.
#[test]
fn hand_typed_title_survives_a_replay_in_the_first_prompt() {
    let dir = Dir::new(
        "title_typed_survives_prompt_replay",
        &[(FAKE, Scenario::Local("title-seed"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "Review the indemnity cap");
    rename(&host, thread, TYPED, false);
    host.settle();
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("title-load-replay-in-prompt"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "And clause 10?");
    assert_eq!(
        renames_in(&events),
        [(TYPED.to_owned(), RenameSource::Lawyer)],
        "{}",
        events.dump()
    );
    let (row, _) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(TYPED), "sidebar row");
    host.shutdown();
}

/// Codex's two titles: the placeholder, then the whole prompt (the agent's title, which replaces
/// the placeholder), then its AI title, which replaces the prompt. The thread ends on the AI
/// title, with the agent as its source, and the log has two renames by the agent.
#[test]
fn codex_prompt_title_then_ai_title_ends_on_the_ai_title() {
    let (dir, host, matter, thread) = two_turns(
        "title_codex_double",
        "title-codex-double",
        FIRST_WORDS,
        true,
    );
    assert_eq!(
        renames(&host, thread),
        [
            (FIRST_WORDS.to_owned(), RenameSource::Placeholder),
            (
                "Review the indemnity cap in clause 9 and tell me if it is market".to_owned(),
                RenameSource::Agent,
            ),
            (AGENT_TITLE.to_owned(), RenameSource::Agent),
        ]
    );
    let (row, header) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(AGENT_TITLE), "sidebar row");
    assert_eq!(header.as_deref(), Some(AGENT_TITLE), "header");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Agent)]);
}

/// The first-words placeholder sent after the agent has named the thread is ignored: the
/// command answers `Done`, and no rename is logged.
#[test]
fn placeholder_is_ignored_once_the_agent_has_named_the_thread() {
    let dir = Dir::new(
        "title_agent_then_placeholder",
        &[(FAKE, Scenario::Local("title-after-rename"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "Read the lease.");
    turn(&host, &mut events, thread, "And clause 10?");
    rename(&host, thread, FIRST_WORDS, true);
    host.settle();
    events.drain_ready();
    assert_eq!(
        renames_in(&events),
        [(AGENT_TITLE.to_owned(), RenameSource::Agent)],
        "{}",
        events.dump()
    );
    let (row, _) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(AGENT_TITLE), "sidebar row");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Agent)]);
}

/// An agent title replaces an earlier agent title; the same one again makes no event.
#[test]
fn agent_title_replaces_an_earlier_agent_title_once() {
    let dir = Dir::new(
        "title_agent_twice",
        &[(FAKE, Scenario::Local("title-turns"))],
    );
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "One.");
    turn(&host, &mut events, thread, "Two.");
    turn(&host, &mut events, thread, "Three.");
    assert_eq!(
        renames_in(&events),
        [
            ("Lease review".to_owned(), RenameSource::Agent),
            ("Lease review v2".to_owned(), RenameSource::Agent),
        ],
        "{}",
        events.dump()
    );
    host.shutdown();
}

/// A hand-typed title sent after an agent title stays when the agent names the thread again.
#[test]
fn typed_title_wins_over_a_later_agent_title() {
    let dir = Dir::new(
        "title_agent_then_typed",
        &[(FAKE, Scenario::Local("title-turns"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "One.");
    rename(&host, thread, TYPED, false);
    // The agent repeats its name, then renames: the typed title stays.
    turn(&host, &mut events, thread, "Two.");
    turn(&host, &mut events, thread, "Three.");
    assert_eq!(
        renames_in(&events),
        [
            ("Lease review".to_owned(), RenameSource::Agent),
            (TYPED.to_owned(), RenameSource::Lawyer),
        ],
        "{}",
        events.dump()
    );
    let (row, _) = titles(&host, matter, thread);
    assert_eq!(row.as_deref(), Some(TYPED), "sidebar row");
    host.shutdown();
    assert_eq!(stored_sources(&dir, matter), [Some(RenameSource::Lawyer)]);
}

/// The agent replays its stored title on load (Codex's replay): the same title as the thread
/// already has from the agent logs nothing new.
#[test]
fn replayed_agent_title_on_load_adds_no_event() {
    let (dir, host, _matter, thread) = two_turns(
        "title_agent_replay",
        "title-after-rename",
        FIRST_WORDS,
        true,
    );
    host.shutdown();

    dir.set_agents(&[(FAKE, Scenario::Local("title-load"))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    turn(&host, &mut events, thread, "And clause 11?");
    assert_eq!(
        renames_in(&events),
        [
            (FIRST_WORDS.to_owned(), RenameSource::Placeholder),
            (AGENT_TITLE.to_owned(), RenameSource::Agent),
        ],
        "{}",
        events.dump()
    );
    host.shutdown();
}
