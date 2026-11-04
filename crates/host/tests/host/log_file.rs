//! The log file's content rule at the host's interface (decision record 0047): host scenarios
//! with the file on (an agent turn that writes, edits and deletes files, an agent that exits
//! mid-turn, one that sends a line cut short, one that sends an update no schema names, a
//! matter whose folder went, a refused folder), then the log is searched for the matter's
//! folder, its file names, the prompt, the agent's text and the titles. It must hold none of
//! them, and at least one lifecycle line and one warning.
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use std::path::Path;

use clauseo_host::log_file;
use clauseo_protocol::{DomainEvent, HostCommand, MatterSource, StreamId, Title};
use clauseo_settings::Locations;

use crate::observe::Events;
use crate::support::{Dir, Scenario, matter_with_thread};
use crate::write;

const WRITER: &str = "fake-writer";
const EXITING: &str = "fake-exiting";
const MALFORMED: &str = "fake-malformed";
const UNKNOWN: &str = "fake-unknown";

/// What the scenarios put in front of the host, none of which may reach the log.
const MATTER_NAME: &str = "Quarrington Holdings";
const PROMPT: &str = "Draft the Zanzibar indemnity clause";
const SECOND_PROMPT: &str = "Summarise the Okonkwo deposition";
const TITLE: &str = "Ptolemy settlement review";
const SEED: &[(&str, &str)] = &[
    ("brief.txt", "Clause 4 draft\n"),
    ("old.txt", "Old terms\n"),
];
/// The agents' words and the files they name (`files-one-turn`, `exit-mid-turn`).
const AGENT_TEXT: &[&str] = &[
    "Created one note, edited the brief and deleted the old file.",
    "Starting on the answer.",
    "Before the broken line.",
    "After the broken line.",
    "agent_mess",
    "Before the unknown update.",
    "_clauseo_test_unknown",
    "stderr filler",
    "Write notes/a.txt",
    "Edit brief.txt",
];
const FILE_NAMES: &[&str] = &["brief.txt", "old.txt", "notes/a.txt", "a.txt"];

fn turn_over(events: &Events) -> bool {
    events.events().any(|event| {
        matches!(
            event,
            DomainEvent::TurnEnded(_) | DomainEvent::TurnFailed(_)
        )
    })
}

fn read_logs(dir: &Path) -> String {
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(dir.join(name)).unwrap_or_default());
    }
    text
}

#[test]
fn the_log_holds_lifecycle_lines_and_no_matter_content() {
    let dir = Dir::new(
        "log_file_content",
        &[
            (WRITER, Scenario::Bundled("files-one-turn")),
            (EXITING, Scenario::Bundled("exit-mid-turn")),
            (MALFORMED, Scenario::Bundled("malformed-line")),
            (UNKNOWN, Scenario::Bundled("unknown-update")),
        ],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let host = dir.start(0);
    // A turn that writes, edits and deletes files, in a titled thread.
    let (matter, thread, folder) = matter_with_thread(&dir, &host, MATTER_NAME, SEED, WRITER);
    host.ok(HostCommand::RenameThread {
        thread,
        title: Title::new(TITLE).unwrap(),
        placeholder: false,
    });
    let mut events = host.events(StreamId::Thread(thread));
    host.prompt(thread, PROMPT);
    events.until("the writer's turn ends", turn_over);

    // An agent that exits mid-turn, with 200 lines of stderr.
    let exiting = host.create_thread(matter, EXITING);
    let mut exits = host.events(StreamId::Thread(exiting));
    host.prompt(exiting, SECOND_PROMPT);
    exits.until("the exiting agent's turn ends", turn_over);

    // An agent that sends a line cut short: a warning, without the line.
    let broken = host.create_thread(matter, MALFORMED);
    let mut breaks = host.events(StreamId::Thread(broken));
    host.prompt(broken, SECOND_PROMPT);
    breaks.until("the malformed agent's turn ends", turn_over);
    // An agent that sends an update of a name no schema has: a warning, without the name.
    let unknown = host.create_thread(matter, UNKNOWN);
    let mut unknowns = host.events(StreamId::Thread(unknown));
    host.prompt(unknown, SECOND_PROMPT);
    unknowns.until("the unknown update's turn ends", turn_over);

    // A refused folder (the data folder itself) and a missing one.
    let refused = host.dispatch(HostCommand::CreateMatter {
        name: Title::new(MATTER_NAME).unwrap(),
        source: MatterSource::ExistingFolder {
            folder: host.folder_ref(dir.data()),
        },
        client: None,
        next_date: None,
    });
    assert!(refused.is_err(), "{refused:?}");
    // The matter's folder goes away; the next prompt finds it missing.
    let moved = folder.with_file_name("moved-away");
    write::rename(&folder, &moved);
    host.prompt(thread, PROMPT);
    events.until("the prompt in a missing folder ends", |e| {
        e.events()
            .filter(|event| {
                matches!(
                    event,
                    DomainEvent::TurnEnded(_) | DomainEvent::TurnFailed(_)
                )
            })
            .count()
            >= 2
    });
    // The rail opening the matter finds the folder gone too.
    drop(host.dispatch(HostCommand::OpenMatter { matter }));
    host.shutdown();
    drop(guard);

    let log = read_logs(&locations.log_dir());
    let canonical = dunce::canonicalize(&moved).unwrap();
    let mut needles: Vec<String> = vec![
        folder.display().to_string(),
        moved.display().to_string(),
        canonical.display().to_string(),
        dir.data().display().to_string(),
        dir.work().display().to_string(),
        MATTER_NAME.to_owned(),
        PROMPT.to_owned(),
        SECOND_PROMPT.to_owned(),
        TITLE.to_owned(),
        "Zanzibar".to_owned(),
    ];
    needles.extend(AGENT_TEXT.iter().map(|text| (*text).to_owned()));
    needles.extend(FILE_NAMES.iter().map(|name| (*name).to_owned()));
    for needle in &needles {
        assert!(
            !log.contains(needle.as_str()),
            "the log holds {needle:?}:\n{log}"
        );
    }
    assert!(
        log.contains("clauseo_host::lifecycle: agent launched"),
        "no lifecycle line:\n{log}"
    );
    assert!(
        log.contains(
            "WARN clauseo_host::actor::turn::updates: the agent sent something unreadable"
        ),
        "no warning reached the log:\n{log}"
    );
}
