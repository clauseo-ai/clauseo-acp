//! A thread's kept model and effort as the store holds them (`threads.own_picks`), written
//! straight into the database between two host runs: a row from before the column (`NULL`)
//! seeds them from the thread's log at its next session start, and the attach writes them
//! (`pickers-kept-null-row`); a row keeping values for two agents and a key that is no agent id
//! sets back only the running agent's (`pickers-kept-per-agent`). Both first run
//! `pickers-kept-luna-first`.

use clauseo_protocol::{StreamId, ThreadId};
use rusqlite::Connection;

use crate::codex::CODEX;
use crate::pickers_idle::started_and_notices;
use crate::pickers_kept::{LUNA_LOW, created, names, run};
use crate::support::{Dir, Scenario};

/// A Codex thread whose config is gpt-6-luna and low ran one prompt, and the host shut down.
fn luna_thread(test: &str) -> (Dir, ThreadId) {
    let dir = Dir::new(test, &[(CODEX, Scenario::Local("pickers-kept-luna-first"))]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let thread = created(&host, matter, Some(&LUNA_LOW));
    let mut events = host.events(StreamId::Thread(thread));
    run(&host, &mut events, thread, "Summarise clause 4.");
    host.shutdown();
    (dir, thread)
}

/// The store's database, opened while no host runs.
fn database(dir: &Dir) -> Connection {
    Connection::open(dir.data().join(clauseo_store::DB_FILE)).unwrap()
}

/// `thread`'s stored own picks.
pub(crate) fn stored(dir: &Dir, thread: ThreadId) -> Option<String> {
    database(dir)
        .query_row(
            "SELECT own_picks FROM threads WHERE id = ?1",
            [thread.to_string()],
            |row| row.get(0),
        )
        .unwrap()
}

/// Sets `thread`'s stored own picks to `value`.
fn store(dir: &Dir, thread: ThreadId, value: Option<&str>) {
    let changed = database(dir)
        .execute(
            "UPDATE threads SET own_picks = ?1 WHERE id = ?2",
            rusqlite::params![value, thread.to_string()],
        )
        .unwrap();
    assert_eq!(changed, 1);
}

/// Restarts the host on `scenario` and runs one prompt on `thread`, which starts on `model` and
/// `effort` with no notice.
fn relaunched(dir: &Dir, thread: ThreadId, scenario: &'static str, model: &str, effort: &str) {
    dir.set_agents(&[(CODEX, Scenario::Local(scenario))]);
    let host = dir.start(1);
    let mut events = host.events(StreamId::Thread(thread));
    events.until_caught_up();
    let second = run(&host, &mut events, thread, "And clause 5?");
    let (started, notices) = started_and_notices(&events, second);
    assert_eq!(started, names(model, effort), "{}", events.dump());
    assert_eq!(notices, [], "{}", events.dump());
    host.shutdown();
}

/// A thread whose row keeps nothing, as a build before the column wrote it: its next session
/// start takes gpt-6-luna and low from what its last session logged, sets them back, and its
/// attach stores them under its agent.
#[test]
fn a_row_from_before_the_column_seeds_from_the_log() {
    let (dir, thread) = luna_thread("pickers_kept_null_row");
    store(&dir, thread, None);
    relaunched(&dir, thread, "pickers-kept-null-row", "GPT-6 Luna", "Low");
    assert_eq!(
        stored(&dir, thread).as_deref(),
        Some(r#"{"codex-acp":{"model":"gpt-6-luna","reasoning_effort":"low"}}"#)
    );
}

/// A row keeping gpt-6-astra and high for codex-acp, gpt-6 and low for claude-acp, and an
/// entry under a key that is no agent id: the host opens, and the relaunch sets codex-acp's
/// values back, neither the other agent's nor the logged gpt-6-luna and low. The other entry
/// stays as it was.
#[test]
fn a_row_keeping_two_agents_values_sets_back_the_running_agents() {
    let (dir, thread) = luna_thread("pickers_kept_per_agent");
    let claude = r#""claude-acp":{"model":"gpt-6","reasoning_effort":"low"}"#;
    let codex = r#""codex-acp":{"model":"gpt-6-astra","reasoning_effort":"high"}"#;
    let row = format!(r#"{{"Not An Agent":{{"model":"gpt-6"}},{claude},{codex}}}"#);
    store(&dir, thread, Some(&row));
    relaunched(
        &dir,
        thread,
        "pickers-kept-per-agent",
        "GPT-6 Astra",
        "High",
    );
    assert_eq!(stored(&dir, thread), Some(format!("{{{claude},{codex}}}")));
}
