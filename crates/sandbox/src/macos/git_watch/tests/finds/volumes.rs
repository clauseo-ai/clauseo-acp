//! Volumes under a watched folder: one that appears while the agent may run ends the session,
//! by an event's scan, a full scan or the 30 s pass, whether the watch could enter it (a local
//! disk) or not (a network share); the final scan keeps it as stuck, which refuses the
//! matter's launches until it is ejected. Volumes that were there at launch are no find.

use std::path::Path;
use std::sync::Arc;

use super::super::super::state::{self, Baseline, Host};
use super::super::super::{Phase, Watch};
use super::super::fake::{
    FakeFs, MATTER, MOVED_IN, MemStore, Notices, aside, event, none, quiet, watch,
};
use crate::DenialSink;

#[test]
fn a_share_or_a_disk_mounted_in_a_live_matter_ends_the_session() {
    for network in [true, false] {
        let fs = FakeFs::new();
        let (mut watch, notices) = watch(&fs, &[]);
        fs.dir("/m/vol");
        if network {
            fs.network("/m/vol");
        } else {
            fs.volume("/m/vol", 7);
        }
        fs.dir("/m/vol/inner");
        let what = if network { "share" } else { "disk" };
        assert!(
            watch.events(&[event("/m/vol", MOVED_IN)]).end,
            "{what}: its event"
        );
        assert!(watch.full_scan(Phase::Running).end, "{what}: a full scan");
        assert!(watch.sweep(&|| false).0.end, "{what}: the 30 s pass");
        none(&notices);
    }
}

#[test]
fn volumes_there_at_launch_are_no_find() {
    let fs = FakeFs::new();
    fs.dir("/m/share");
    fs.network("/m/share");
    fs.dir("/m/disk");
    fs.volume("/m/disk", 7);
    fs.dir("/m/disk/inner");
    let (mut watch, notices) = watch(&fs, &[]);
    quiet(watch.events(&[event("/m/share", MOVED_IN), event("/m/disk", MOVED_IN)]));
    quiet(watch.full_scan(Phase::Running));
    fs.dir("/m/disk/new");
    quiet(watch.sweep(&|| false).0);
    quiet(watch.full_scan(Phase::Ended));
    assert_eq!(watch.take_stuck().len(), 0);
    none(&notices);
}

#[test]
fn a_record_made_before_volumes_were_kept_judges_none_new() {
    let fs = FakeFs::new();
    let notices = Arc::new(Notices::default());
    let mut watch = Watch::resume(
        fs.clone(),
        MATTER.into(),
        aside(&notices),
        Baseline::default(),
    );
    fs.dir("/m/share");
    fs.network("/m/share");
    quiet(watch.full_scan(Phase::Ended));
    assert_eq!(watch.take_stuck().len(), 0);
}

#[test]
fn a_volume_left_mounted_after_the_session_refuses_launches_until_it_is_ejected() {
    let fs = FakeFs::new();
    let (mut watch, _notices) = watch(&fs, &[]);
    fs.dir("/m/share");
    fs.network("/m/share");
    quiet(watch.full_scan(Phase::Ended));
    let stuck = watch.take_stuck();
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!(stuck[0].mount && !stuck[0].unreadable);
    let store = MemStore::default();
    state::end(&store, "s", stuck);
    let sink = Arc::new(Notices::default()) as Arc<dyn DenialSink>;
    let host = Host {
        dead: &|_| false,
        kill: &|_| true,
    };
    let begin = || state::begin(&store, &fs, Path::new(MATTER), &sink, &host);
    let error = begin().unwrap_err().to_string();
    assert!(
        error.starts_with(
            "Clauseo won't start an AI tool in this matter until the disk or share mounted at \
             share is ejected"
        ),
        "{error}"
    );
    assert!(begin().is_err(), "still mounted");
    fs.eject("/m/share");
    begin().unwrap();
}
