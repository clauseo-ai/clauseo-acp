//! The log line of a launch that skipped a skill pack (decision record 0040, 0047): the pack's
//! id and reason code, with no path and none of the pack's text. The log is the process's global
//! subscriber, so this module holds one test: nextest runs it in a process of its own.

use clauseo_host::log_file;
use clauseo_settings::Locations;
use serde_json::json;

use crate::packs::{
    exits_on_prompt, pack_dir, prompt_until_over, put_pack, skills_on, start, warnings,
};
use crate::profile_rig::{CLAUDE, Wired, agents, settings};
use crate::support::Dir;
use crate::write;

/// What the pack holds that the log must not: its name, its description and its body.
const PACK_TEXT: &[&str] = &["A pack", "Example Chambers", "Reviews a", "# nda-review"];

#[test]
fn a_changed_pack_is_skipped_with_a_notice_and_a_log_line_holding_no_path() {
    let dir = Dir::new("packs_log_line", &[]);
    let logs = dir.data().join("logs-under-test");
    write::create_dir_all(&logs);
    let locations = Locations::under_root(&logs);
    let guard = log_file::install(&locations);
    assert!(guard.is_active(), "the log did not start");

    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    write::write(&pack_dir(&dir, "nda-kit").join("README.md"), b"# Changed\n");
    let looping = exits_on_prompt(&dir, "packs-log");
    settings(
        &dir,
        agents(&[(CLAUDE, &looping, json!({}))]),
        skills_on(&[("nda-kit", &hash)]),
    );
    let launcher = Wired::new(&dir);
    let host = start(&dir, 0, &launcher);
    let folder = dir.folder("Quarrington Holdings", &[]);
    let matter = host.create_matter("Quarrington Holdings", &folder);
    let thread = host.create_thread(matter, CLAUDE);
    let events = prompt_until_over(&host, thread);
    prompt_until_over(&host, thread);
    host.shutdown();
    assert_eq!(
        warnings(&events),
        [
            "Clauseo didn't load the skill pack \"nda-kit\". Its files changed after it was turned on. Turn it on again to use it."
        ]
    );
    assert_eq!(launcher.seen()[0].packs, Vec::<String>::new());

    guard.flush();
    let mut text = String::new();
    for name in ["host.2.log", "host.1.log", "host.log"] {
        text.push_str(&std::fs::read_to_string(locations.log_dir().join(name)).unwrap_or_default());
    }
    // One line per launch, not per pack: two launches, two lines.
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("skill packs were left out"))
        .collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines.iter().all(|line| line.contains("nda-kit:changed")),
        "{lines:?}"
    );
    let data = dir.data().to_str().unwrap();
    let folder = folder.to_str().unwrap();
    for needle in [data, folder, "skill-packs/", "Quarrington"]
        .iter()
        .chain(PACK_TEXT)
    {
        assert!(!text.contains(needle), "the log holds {needle:?}:\n{text}");
    }
}
