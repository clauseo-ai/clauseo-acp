//! What can't move is neutralised where it is; repositories completed after they arrive;
//! link-shaped `objects` and `refs`; and a launch repository's `commondir` target.

use std::path::PathBuf;

use super::super::{Launch, Phase, flags};
use super::fake::{
    FakeFs, MOVED_IN, assert_set_aside, event, found, none, quiet, watch, watch_launch,
};
use crate::{Denial, GitItem, GitOutcome};

const CREATED_FILE: u32 = flags::ITEM_CREATED | 0x1_0000;
const CREATED_DIR: u32 = flags::ITEM_CREATED | 0x2_0000;

/// The one notice's outcome.
fn outcome(notices: &super::fake::Notices) -> GitOutcome {
    let notices = notices.taken();
    let [Denial::GitSetAside { outcome, .. }] = notices.as_slice() else {
        panic!("{notices:?}");
    };
    outcome.clone()
}

#[test]
fn what_cannot_move_loses_its_head_and_config_under_random_names() {
    let fs = FakeFs::new();
    fs.fail_moves(18); // EXDEV
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/bare");
    fs.bare("/m/bare");
    fs.file("/m/bare/config");
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/bare/objects") && !fs.exists("/m/bare/HEAD"));
    assert!(!fs.exists("/m/bare/config"));
    let heads = fs.named("/m/bare", "HEAD-set-aside-by-clauseo-");
    assert_eq!(heads.len(), 1, "{heads:?}");
    let suffix = heads[0]
        .to_string_lossy()
        .rsplit('-')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(suffix.len(), 16, "a random suffix: {suffix}");
    assert_eq!(fs.named("/m/bare", "config-set-aside-by-clauseo-").len(), 1);
    assert_eq!(
        outcome(&notices),
        GitOutcome::RenamedInPlace(heads[0].clone())
    );
    // A .git folder loses its HEAD and config and its own name.
    fs.repo("/m/.git");
    fs.file("/m/.git/config");
    quiet(watch.full_scan(Phase::Ended));
    let renamed = fs.named("/m", ".git-set-aside-by-clauseo-");
    assert_eq!(renamed.len(), 1, "{renamed:?}");
    let inside = renamed[0].to_string_lossy().into_owned();
    assert_eq!(fs.named(&inside, "HEAD-set-aside-by-clauseo-").len(), 1);
    assert_eq!(fs.named(&inside, "config-set-aside-by-clauseo-").len(), 1);
    assert_eq!(
        outcome(&notices),
        GitOutcome::RenamedInPlace(renamed[0].clone())
    );
    let records = fs.records();
    assert!(
        records
            .iter()
            .all(|r| r["error"].as_str().unwrap().contains("os error 18"))
    );
}

#[test]
fn a_link_out_that_cannot_move_is_removed_with_its_target_recorded() {
    let fs = FakeFs::new();
    fs.fail_moves(18);
    fs.dir("/state");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.link("/m/out", "/state");
    quiet(watch.full_scan(Phase::Ended));
    assert!(!fs.exists("/m/out") && fs.exists("/state"));
    assert_eq!(
        outcome(&notices),
        GitOutcome::LinkRemoved {
            target: PathBuf::from("/state")
        }
    );
    assert_eq!(fs.records()[0]["link_target"], "/state");
}

#[test]
fn what_can_neither_move_nor_be_renamed_is_kept_as_stuck() {
    let fs = FakeFs::new();
    fs.fail_moves(18);
    fs.fail_in_place();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.repo("/m/.git");
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/.git/HEAD"));
    assert_eq!(outcome(&notices), GitOutcome::Stuck);
    let stuck = watch.take_stuck();
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].path, PathBuf::from("/m/.git"));
    assert!(
        stuck[0].reason.contains("os error 18"),
        "{}",
        stuck[0].reason
    );
}

#[test]
fn a_folder_with_only_head_and_config_moved_in_loses_its_head_at_once() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/half");
    fs.file("/m/half/HEAD");
    fs.file("/m/half/config");
    found(&mut watch, &[event("/m/half", MOVED_IN)]);
    assert!(fs.exists("/m/half/config") && !fs.exists("/m/half/HEAD"));
    assert_set_aside(&notices, "half/HEAD", GitItem::BareRepoHead);
}

#[test]
fn objects_arriving_in_a_launch_head_folder_end_the_session_by_name() {
    let fs = FakeFs::new();
    fs.dir("/m/half");
    fs.file("/m/half/HEAD");
    fs.file("/m/half/config");
    let (mut watch, notices) = watch(&fs, &[]);
    // At launch a folder with a HEAD is the lawyer's while it stays that way.
    fs.file("/m/half/notes.txt");
    quiet(watch.events(&[event("/m/half/notes.txt", CREATED_FILE)]));
    // An objects folder arriving beside the HEAD ends the session by its name, before the
    // folder is repository-shaped.
    fs.dir("/m/half/objects");
    assert!(watch.events(&[event("/m/half/objects", CREATED_DIR)]).end);
    // Not shaped, the folder stays the lawyer's: the final scan leaves it.
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    // With refs too it is a repository: its HEAD goes.
    fs.dir("/m/half/refs");
    found(&mut watch, &[event("/m/half/refs", CREATED_DIR)]);
    assert!(fs.exists("/m/half/objects") && !fs.exists("/m/half/HEAD"));
    assert_set_aside(&notices, "half/HEAD", GitItem::BareRepoHead);
}

#[test]
fn a_file_arriving_beside_a_missed_head_brings_it_to_light() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/new");
    quiet(watch.events(&[event("/m/new", CREATED_DIR)]));
    // The HEAD's own event never came (coalesced away); a commondir file's does.
    fs.file("/m/new/HEAD");
    fs.file("/m/new/commondir");
    found(&mut watch, &[event("/m/new/commondir", CREATED_FILE)]);
    assert!(!fs.exists("/m/new"));
    assert_set_aside(&notices, "new", GitItem::BareRepo);
}

#[test]
fn objects_and_refs_that_are_links_make_the_repository_shape() {
    let fs = FakeFs::new();
    fs.dir("/state");
    fs.dir("/state/objects");
    fs.dir("/state/refs");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/linked");
    fs.file("/m/linked/HEAD");
    fs.link("/m/linked/objects", "/state/objects");
    fs.link("/m/linked/refs", "/state/refs");
    found(&mut watch, &[event("/m/linked", MOVED_IN)]);
    assert!(!fs.exists("/m/linked"));
    assert_set_aside(&notices, "linked", GitItem::BareRepo);
}

#[test]
fn what_is_made_at_a_launch_commondir_target_is_set_aside() {
    let fs = FakeFs::new();
    fs.file("/m/config");
    let launch = Launch {
        common: ["objects", "refs", "config"]
            .iter()
            .map(|name| PathBuf::from("/m").join(name))
            .collect(),
        ..Launch::default()
    };
    let (mut watch, notices) = watch_launch(&fs, &launch);
    // What was there at launch stays; a new objects folder goes.
    fs.file("/m/notes.txt");
    quiet(watch.events(&[event("/m/notes.txt", CREATED_FILE)]));
    fs.dir("/m/objects");
    fs.dir("/m/objects/pack");
    found(&mut watch, &[event("/m/objects/pack", CREATED_DIR)]);
    assert!(!fs.exists("/m/objects") && fs.exists("/m/config"));
    assert_set_aside(&notices, "objects", GitItem::CommonDirPart);
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
}
