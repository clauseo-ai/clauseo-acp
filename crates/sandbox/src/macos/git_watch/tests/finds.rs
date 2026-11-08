//! Which events end the session: a git name in an event's path, whether or not its item is
//! still there, checked before any folder is read; the count of items gone by check time; the
//! scan budget of arrived folders; what a final scan moves; and a network volume in the
//! matter.

use std::path::PathBuf;

use super::super::check::{SCAN_BUDGET, gone_total};
use super::super::state::Baseline;
use super::super::{Phase, Watch, flags};
use super::fake::{
    FakeFs, MATTER, MOVED_IN, Notices, aside, assert_moved, assert_set_aside, event, none, quiet,
    watch,
};
use crate::{Denial, GitItem};

const CREATED_FILE: u32 = flags::ITEM_CREATED | 0x1_0000;
const MODIFIED_FILE: u32 = 0x1000 | 0x1_0000;

#[test]
fn a_repository_renamed_in_and_out_before_the_check_still_ends_the_session() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    // A folder holding a .git came in, git wrote in it, and it went again: nothing is left.
    let events = [
        event("/m/foo", MOVED_IN),
        event("/m/foo/.git/index", CREATED_FILE),
    ];
    let before = gone_total();
    let asks = watch.events(&events);
    assert!(asks.end, "{asks:?}");
    assert_eq!(watch.gone(), 2);
    assert!(gone_total() >= before + 2);
    // A bare repository likewise, by its HEAD.
    let events = [event("/m/r", MOVED_IN), event("/m/r/HEAD", MODIFIED_FILE)];
    assert!(watch.events(&events).end);
    // An objects folder arriving beside a HEAD, gone too.
    fs.dir("/m/half");
    fs.file("/m/half/HEAD");
    let events = [event("/m/half/objects", MOVED_IN)];
    assert!(watch.events(&events).end);
    quiet(watch.full_scan(Phase::Ended));
    // Nothing of the lawyer's was moved for a find that was gone: only the HEAD left.
    assert_set_aside(&notices, "half/HEAD", GitItem::BareRepoHead);
}

#[test]
fn a_batch_with_a_name_hit_lists_no_folder() {
    let fs = FakeFs::new();
    fs.dir("/m/docs");
    let (mut watch, _notices) = watch(&fs, &[]);
    fs.dir("/m/big");
    fs.dir("/m/big/inner");
    fs.repo("/m/big/inner/.git");
    let listed = fs.listings();
    let events = [
        event("/m/big", MOVED_IN),
        event("/m/docs/HEAD", CREATED_FILE),
    ];
    assert!(watch.events(&events).end);
    assert_eq!(
        fs.listings(),
        listed,
        "a name hit ends the session before any listing"
    );
    // Without one, the arrived folder is listed and its .git found.
    assert!(watch.events(&[event("/m/big", MOVED_IN)]).end);
    assert!(fs.listings() > listed);
}

#[test]
fn an_arrived_item_gone_at_check_time_is_counted_and_nothing_else() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    let before = gone_total();
    quiet(watch.events(&[event("/m/tmp-1234", MOVED_IN)]));
    quiet(watch.events(&[event("/m/gone.txt", 0x200 | 0x1_0000)]));
    assert_eq!(watch.gone(), 1, "a removal is not an arrival");
    assert!(gone_total() > before);
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
}

#[test]
fn a_launch_repository_is_no_name_hit_while_it_keeps_its_file_id() {
    let fs = FakeFs::new();
    fs.repo("/m/.git");
    fs.dir("/m/notes");
    fs.file("/m/notes/HEAD");
    let (mut watch, notices) = watch(&fs, &["/m/.git"]);
    // git at work in the launch repository, and a launch folder's own HEAD rewritten.
    let events = [
        event("/m/.git/index.lock", CREATED_FILE),
        event("/m/.git/HEAD", MODIFIED_FILE),
        event("/m/notes/HEAD", MODIFIED_FILE),
    ];
    quiet(watch.events(&events));
    // Gone, it is no longer the launch's: its name ends the session.
    fs.remove("/m/.git");
    assert!(watch.events(&[event("/m/.git/index", CREATED_FILE)]).end);
    none(&notices);
}

#[test]
fn an_arrived_folder_past_the_scan_budget_ends_the_session_and_the_final_scan_has_none() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/big");
    for n in 0..=SCAN_BUDGET {
        fs.file(&format!("/m/big/f{n}"));
    }
    // Nothing git in it: past the budget alone, the session ends.
    assert!(watch.events(&[event("/m/big", MOVED_IN)]).end);
    fs.dir("/m/big/sub");
    fs.repo("/m/big/sub/.git");
    watch.ended_session();
    quiet(watch.full_scan(Phase::Ended));
    // The final scan reached the .git; only it moved, not the folder around it.
    assert_moved(&notices, "big/sub/.git");
    assert!(fs.exists("/m/big/sub") && fs.exists("/m/big/f0"));
}

#[test]
fn a_dead_sessions_scan_moves_repository_shapes_only() {
    let fs = FakeFs::new();
    let notices = std::sync::Arc::new(Notices::default());
    let mut watch = Watch::resume(
        fs.clone(),
        PathBuf::from(MATTER),
        aside(&notices),
        Baseline::default(),
    );
    // Made since: a working tree with its .git, and a bare repository.
    fs.dir("/m/proj");
    fs.file("/m/proj/brief.docx");
    fs.repo("/m/proj/.git");
    fs.dir("/m/archive");
    fs.bare("/m/archive");
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/proj/brief.docx") && !fs.exists("/m/proj/.git"));
    assert!(fs.exists("/m/archive/objects") && !fs.exists("/m/archive/HEAD"));
    let mut got: Vec<(PathBuf, GitItem)> = notices
        .taken()
        .into_iter()
        .map(|d| {
            let Denial::GitSetAside { path, what, .. } = d else {
                panic!("{d:?}");
            };
            (path, what)
        })
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        [
            (PathBuf::from("archive/HEAD"), GitItem::BareRepoHead),
            (PathBuf::from("proj/.git"), GitItem::DotGit),
        ]
    );
}

#[test]
fn a_network_volume_in_the_matter_is_never_entered_but_a_name_still_ends_the_session() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/share");
    fs.repo("/m/share/.git");
    fs.network("/m/share");
    quiet(watch.full_scan(Phase::Ended));
    // An event naming a .git ends the session before anything is read, on any volume.
    assert!(watch.events(&[event("/m/share/.git", MOVED_IN)]).end);
    quiet(watch.events(&[event("/m/share", MOVED_IN)]));
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/share/.git"));
    none(&notices);
}
