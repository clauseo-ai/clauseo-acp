//! The line a fresh data folder's first conversation logs for a pick the agent does not take
//! (`actor/turn/picks.rs`): the same lifecycle line as any stale pick. Its model is `-`, since
//! `loggable` writes only a value the agent lists, and its effort `-`, since no effort was picked.
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::{AgentId, CommandOutcome, HostCommand};
use clauseo_settings::Locations;

use crate::pickers::{CLAUDE, current, logged, picks, turn};
use crate::support::{Dir, Scenario};
use crate::write;

#[test]
fn an_unknown_pick_on_a_fresh_folder_is_logged_and_the_default_runs() {
    let dir = Dir::new(
        "pickers_seed_log",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let config = picks(&[("model", "claude-gone-9")]);
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config: Some(config),
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    let events = turn(&host, thread, "Summarise clause 4.");
    assert_eq!(current(&logged(&events), "model").as_deref(), Some("opus"));
    host.settle();
    host.shutdown();
    drop(guard);

    let mut log = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        log.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    let dropped: Vec<&str> = log
        .lines()
        .filter_map(|line| {
            let at = line.find("the agent does not take a config pick")?;
            Some(&line[at..])
        })
        .collect();
    assert_eq!(
        dropped,
        ["the agent does not take a config pick; dropped agent=claude-acp model=- effort=-"],
        "{log}"
    );
    assert!(
        !log.contains("claude-gone-9"),
        "an unlisted value is never logged: {log}"
    );
    assert!(!log.contains("picks sent before first prompt"), "{log}");
}
