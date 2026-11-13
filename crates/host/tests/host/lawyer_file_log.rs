//! The lifecycle lines of a turn's end and of its Undo: counts only, never a path. The dry run's
//! shape: the lawyer saves a note while the agent reads, so the turn changed two papers, one
//! flagged; the default Undo keeps the note and moves the agent's file to the trash.
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::{
    AgentChoiceId, FileChoice, FileDecision, HostCommand, MatterPath, StopReason, StreamId,
};
use clauseo_settings::Locations;

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};
use crate::write;

const NOTE: &str = "Lawyer note.txt";

#[test]
fn a_turn_s_end_and_its_undo_log_counts_and_no_path() {
    let dir = Dir::new(
        "lawyer_file_log",
        &[(FAKE, Scenario::Local("undo-lawyer-file-read"))],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Read every paper.");
    events.until("the read's permission request", |e| {
        !e.permission_requests().is_empty()
    });
    write::write(&folder.join(NOTE), b"Call the client about clause 4.\n");
    host.ok(HostCommand::AnswerPermission {
        thread,
        request: events.permission_requests()[0],
        option_id: AgentChoiceId::new("allow-once").unwrap(),
    });
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: vec![
            FileDecision {
                path: MatterPath::new(NOTE).unwrap(),
                choice: FileChoice::Keep,
            },
            FileDecision {
                path: MatterPath::new("agent.txt").unwrap(),
                choice: FileChoice::Undo,
            },
        ],
    });
    assert!(!folder.join("agent.txt").exists());
    host.settle();
    host.shutdown();
    drop(guard);
    let mut log = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        log.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    let line = |message: &str| {
        log.lines()
            .find(|line| line.contains(message))
            .unwrap_or_else(|| panic!("no {message:?} line: {log}"))
            .to_owned()
    };
    let ended = line("clauseo_host::lifecycle: a turn ended");
    for field in ["stop=EndTurn", "changed=2", "flagged=1"] {
        assert!(ended.contains(field), "{field} in {ended}");
    }
    let undo = line("clauseo_host::lifecycle: an undo ran");
    for field in ["undone=1", "kept=1", "trashed=1", "deleted=0"] {
        assert!(undo.contains(field), "{field} in {undo}");
    }
    let folder_text = folder.to_string_lossy().into_owned();
    for line in [&ended, &undo] {
        for path in [folder_text.as_str(), NOTE, "agent.txt", "brief.txt"] {
            assert!(!line.contains(path), "no path in {line}");
        }
    }
}
