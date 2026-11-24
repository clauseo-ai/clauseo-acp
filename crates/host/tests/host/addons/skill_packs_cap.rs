//! `ListSkillPacks` over a packs folder holding many folders (decision record 0040, "Turning
//! packs on (PR-E)", Cost): the folder is listed once, so the list is linear in the folders, and
//! at most 256 packs, the first by id, are read and answered; the rest are logged by count
//! alone. The log is the process's global subscriber, so the log test installs it; nextest runs
//! each test in a process of its own.

use std::time::Duration;

use clauseo_host::log_file;
use clauseo_protocol::{HostQuery, PackStatus, QueryResult};
use clauseo_settings::Locations;

use crate::packs_timing::now;
use crate::support::{Dir, Running};
use crate::write;

/// `count` empty folders `pack-0000`, `pack-0001`, ... in the packs folder: each a `PackId`,
/// each invalid when read (no manifest), so the reads are cheap and the listing is the cost.
fn folders(dir: &Dir, count: usize) {
    for at in 0..count {
        let folder = dir.data().join("skill-packs").join(format!("pack-{at:04}"));
        write::create_dir_all(&folder);
    }
}

fn start(dir: &Dir) -> Running {
    dir.set_settings(&[], serde_json::json!({"skills": {"extra": "any"}}));
    dir.start(0)
}

fn list(host: &Running) -> Vec<PackStatus> {
    match host.query(HostQuery::ListSkillPacks).unwrap() {
        QueryResult::SkillPacks(packs) => packs,
        other => panic!("ListSkillPacks answered {other:?}"),
    }
}

#[test]
fn more_than_256_skill_pack_folders_list_256_and_log_the_rest_by_count() {
    let dir = Dir::new("skill_packs_cap", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");
    folders(&dir, 300);
    let host = start(&dir);
    let listed = list(&host);
    host.shutdown();
    assert_eq!(listed.len(), 256);
    let ids: Vec<String> = listed.iter().map(|s| s.id.as_str().to_owned()).collect();
    let first: Vec<String> = (0..256).map(|at| format!("pack-{at:04}")).collect();
    assert_eq!(ids, first, "the first 256 by name");

    guard.flush();
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("past the list's cap"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("44"), "{lines:?}");
    for needle in [
        "pack-0256",
        "pack-0299",
        "skill-packs",
        dir.data().to_str().unwrap(),
    ] {
        assert!(!text.contains(needle), "the log holds {needle:?}:\n{text}");
    }
}

/// A guard against the list going quadratic again: listing the folder once per pack read made
/// 1,000 folders cost about half a million directory entries; one listing is a millisecond.
#[test]
fn listing_skill_packs_over_1000_folders_is_quick() {
    let dir = Dir::new("skill_packs_quick", &[]);
    folders(&dir, 1000);
    let host = start(&dir);
    let started = now();
    let listed = list(&host);
    let took = now().duration_since(started);
    host.shutdown();
    assert_eq!(listed.len(), 256);
    assert!(took < Duration::from_secs(1), "the list took {took:?}");
}
