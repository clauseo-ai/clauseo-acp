//! The pickers' lines in host.log (`actor/turn/picks.rs`): the picks a session start set, and
//! the ones the agent does not take, each as one lifecycle line with the agent id, the model and
//! the effort and nothing else (no thread, path, prompt, title or matter name).
//!
//! The log is the process's global subscriber, so this module holds one test: nextest runs it
//! in a process of its own.

use clauseo_host::log_file;
use clauseo_protocol::FieldChange;
use clauseo_settings::Locations;

use crate::pickers::{CLAUDE, claude_prefs, create_thread, picks, turn};
use crate::support::{Dir, Scenario};
use crate::write;

/// The one line of `log` that holds `message`, from the message on.
fn line<'a>(log: &'a str, message: &str) -> &'a str {
    let mut found = log
        .lines()
        .filter_map(|line| line.find(message).map(|at| &line[at..]));
    let first = found
        .next()
        .unwrap_or_else(|| panic!("no {message:?} line in\n{log}"));
    assert!(found.next().is_none(), "two {message:?} lines in\n{log}");
    first
}

#[test]
fn the_picks_set_and_dropped_are_logged_with_the_agent_model_and_effort_only() {
    let dir = Dir::new(
        "pickers_log",
        &[(CLAUDE, Scenario::Bundled("pickers-picked"))],
    );
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    // `CreateThread`'s config, set before the first prompt.
    let host = dir.start(0);
    let folder = dir.folder("Lease review", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("Lease review", &folder);
    let config = picks(&[("model", "opus"), ("effort", "high")]);
    let thread = create_thread(&host, matter, Some(config));
    drop(turn(&host, thread, "Summarise clause 4."));
    host.shutdown();

    // The matter's prefs, which the agent does not take: opus refused, max not offered.
    dir.set_agents(&[(CLAUDE, Scenario::Bundled("pickers-stale"))]);
    let host = dir.start(1);
    let stale = claude_prefs(&[("model", "opus"), ("effort", "max")]);
    host.ok(clauseo_protocol::HostCommand::UpdateMatter {
        matter,
        client: FieldChange::Keep,
        next_date: FieldChange::Keep,
        skill_packs: FieldChange::Keep,
        pinned: None,
        agent_prefs: FieldChange::Set(stale),
    });
    let second = create_thread(&host, matter, None);
    drop(turn(&host, second, "Summarise clause 5."));
    host.settle();
    host.shutdown();
    drop(guard);

    let mut log = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        log.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    assert_eq!(
        line(&log, "picks sent before first prompt"),
        "picks sent before first prompt agent=claude-acp model=opus effort=high"
    );
    assert_eq!(
        line(&log, "the agent does not take a config pick"),
        // The session does not list max, so it shows as `-` (record 0047: listed short ids only).
        "the agent does not take a config pick; dropped agent=claude-acp model=opus effort=-"
    );
    for line in log
        .lines()
        .filter(|line| line.contains("config pick") || line.contains("picks"))
    {
        assert!(line.contains("clauseo_host::lifecycle: "), "{line}");
        for kept_out in [
            thread.to_string(),
            second.to_string(),
            "Lease review".to_owned(),
            "Summarise clause".to_owned(),
            folder.display().to_string(),
        ] {
            assert!(!line.contains(&kept_out), "{line} holds {kept_out}");
        }
    }
}
