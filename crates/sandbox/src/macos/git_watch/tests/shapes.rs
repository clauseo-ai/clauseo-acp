//! The repository and link shapes: a bare layout (new, or in a folder that existed), a lone
//! `HEAD`, a clonefile'd tree, links out of the matter, and the words the agent and the lawyer
//! get, with every path shown made safe.

use std::path::PathBuf;

use super::super::{Phase, flags};
use super::fake::{
    FakeFs, MOVED_IN, assert_moved, assert_set_aside, event, found, none, quiet, watch, watch_with,
};
use crate::{Denial, GitItem, GitOutcome};

const CREATED_FILE: u32 = flags::ITEM_CREATED | 0x1_0000;
const CREATED_LINK: u32 = flags::ITEM_CREATED | 0x4_0000;

#[test]
fn a_new_folder_laid_out_as_a_bare_repository_is_set_aside_whole() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.dir("/m/foo/bar");
    fs.bare("/m/foo/bar");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    assert!(!fs.exists("/m/foo/bar") && fs.exists("/m/foo"));
    assert_set_aside(&notices, "foo/bar", GitItem::BareRepo);
    let kept = fs.set_aside();
    assert!(kept.iter().any(|p| p.ends_with("item/objects")), "{kept:?}");
    // The moved-in folder itself.
    fs.dir("/m/plain");
    fs.bare("/m/plain");
    found(&mut watch, &[event("/m/plain", MOVED_IN)]);
    assert!(!fs.exists("/m/plain"));
    assert_set_aside(&notices, "plain", GitItem::BareRepo);
}

#[test]
fn a_head_made_in_a_folder_that_existed_is_set_aside_alone() {
    let fs = FakeFs::new();
    fs.dir("/m/Clients");
    fs.file("/m/Clients/brief.txt");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.bare("/m/Clients");
    found(&mut watch, &[event("/m/Clients/HEAD", CREATED_FILE)]);
    assert!(!fs.exists("/m/Clients/HEAD"));
    assert!(fs.exists("/m/Clients/brief.txt") && fs.exists("/m/Clients/objects"));
    assert_set_aside(&notices, "Clients/HEAD", GitItem::BareRepoHead);
    // A lone HEAD, with no layout around it, goes too, in any letter case.
    fs.file("/m/head");
    found(&mut watch, &[event("/m/head", CREATED_FILE)]);
    assert_set_aside(&notices, "head", GitItem::BareRepoHead);
}

#[test]
fn a_head_that_is_a_link_counts_as_a_head() {
    let fs = FakeFs::new();
    fs.dir("/m/old");
    let (mut watch, notices) = watch(&fs, &[]);
    // A new folder whose HEAD is a link (git's old layout) goes whole.
    fs.dir("/m/new");
    fs.dir("/m/new/objects");
    fs.dir("/m/new/refs");
    fs.dir("/m/new/refs/heads");
    fs.file("/m/new/refs/heads/main");
    fs.link("/m/new/HEAD", "/m/new/refs/heads/main");
    found(&mut watch, &[event("/m/new", MOVED_IN)]);
    assert!(!fs.exists("/m/new"));
    assert_set_aside(&notices, "new", GitItem::BareRepo);
    // A HEAD link made in a folder that existed goes alone, though it leads inside.
    fs.file("/m/old/main");
    fs.link("/m/old/HEAD", "/m/old/main");
    found(&mut watch, &[event("/m/old/HEAD", CREATED_LINK)]);
    assert!(!fs.exists("/m/old/HEAD") && fs.exists("/m/old/main"));
    assert_set_aside(&notices, "old/HEAD", GitItem::BareRepoHead);
}

#[test]
fn a_head_with_a_commondir_is_repository_shaped() {
    let fs = FakeFs::new();
    fs.dir("/m/old");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/wt");
    fs.file("/m/wt/HEAD");
    fs.file("/m/wt/commondir");
    found(&mut watch, &[event("/m/wt", MOVED_IN)]);
    assert!(!fs.exists("/m/wt"));
    assert_set_aside(&notices, "wt", GitItem::BareRepo);
    // In a folder that existed, the exit scan takes its HEAD only.
    fs.file("/m/old/commondir");
    fs.file("/m/old/HEAD");
    quiet(watch.full_scan(Phase::Ended));
    assert!(!fs.exists("/m/old/HEAD") && fs.exists("/m/old/commondir"));
    assert_set_aside(&notices, "old/HEAD", GitItem::BareRepoHead);
    // HEAD with a commondir folder, or objects alone, is not the shape: the folder stays, and
    // its HEAD goes, as any HEAD in a folder that was not a repository at launch does.
    fs.dir("/m/not");
    fs.file("/m/not/HEAD");
    fs.dir("/m/not/commondir");
    fs.dir("/m/not/objects");
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/not/objects") && !fs.exists("/m/not/HEAD"));
    assert_set_aside(&notices, "not/HEAD", GitItem::BareRepoHead);
}

#[test]
fn a_head_that_completes_a_new_folder_takes_the_folder() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/new");
    quiet(watch.events(&[event("/m/new", flags::ITEM_CREATED)]));
    fs.bare("/m/new");
    found(&mut watch, &[event("/m/new/HEAD", CREATED_FILE)]);
    assert!(!fs.exists("/m/new"));
    assert_set_aside(&notices, "new", GitItem::BareRepo);
}

#[test]
fn the_exit_scan_takes_new_repositories_whole_and_old_folders_head() {
    let fs = FakeFs::new();
    fs.dir("/m/old");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.bare("/m/old");
    fs.dir("/m/new");
    fs.bare("/m/new");
    quiet(watch.full_scan(Phase::Ended));
    assert!(fs.exists("/m/old/objects") && !fs.exists("/m/old/HEAD"));
    assert!(!fs.exists("/m/new"));
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
            (PathBuf::from("new"), GitItem::BareRepo),
            (PathBuf::from("old/HEAD"), GitItem::BareRepoHead),
        ]
    );
}

#[test]
fn the_launch_bare_repository_is_left_alone() {
    let fs = FakeFs::new();
    fs.dir("/m/archive.git");
    fs.bare("/m/archive.git");
    let (mut watch, notices) = watch(&fs, &[]);
    // git rewrites HEAD by a rename of HEAD.lock.
    fs.file("/m/archive.git/HEAD.lock");
    let events = [
        event("/m/archive.git/HEAD", flags::ITEM_RENAMED),
        event("/m/archive.git/refs", flags::ITEM_CREATED),
    ];
    quiet(watch.events(&events));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    assert!(fs.exists("/m/archive.git/HEAD"));
}

#[test]
fn a_clonefiled_tree_holding_a_git_is_set_aside() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    // `cp -c -R`: one clonefile of the folder, reported as a created, cloned folder.
    fs.dir("/m/copy");
    fs.dir("/m/copy/src");
    fs.repo("/m/copy/.git");
    let cloned = flags::ITEM_CREATED | flags::ITEM_CLONED | 0x2_0000;
    found(&mut watch, &[event("/m/copy", cloned)]);
    assert!(!fs.exists("/m/copy/.git") && fs.exists("/m/copy/src"));
    assert_moved(&notices, "copy/.git");
}

#[test]
fn a_link_out_of_the_matter_or_nowhere_is_set_aside() {
    let fs = FakeFs::new();
    fs.dir("/state");
    fs.dir("/state/foo");
    fs.repo("/state/foo/.git");
    fs.dir("/m/docs");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.link("/m/foo", "/state/foo");
    found(&mut watch, &[event("/m/foo", CREATED_LINK)]);
    assert!(!fs.exists("/m/foo") && fs.exists("/state/foo/.git"));
    assert_set_aside(&notices, "foo", GitItem::OutsideLink);
    fs.link("/m/broken", "/nowhere");
    found(&mut watch, &[event("/m/broken", CREATED_LINK)]);
    assert_set_aside(&notices, "broken", GitItem::OutsideLink);
    // One in a moved-in folder.
    fs.dir("/m/in");
    fs.link("/m/in/up", "/state");
    found(&mut watch, &[event("/m/in", MOVED_IN)]);
    assert_set_aside(&notices, "in/up", GitItem::OutsideLink);
}

#[test]
fn a_link_inside_the_matter_and_the_launch_links_stay() {
    let fs = FakeFs::new();
    fs.dir("/m/docs");
    fs.dir("/outside");
    fs.link("/m/dropbox", "/outside");
    let (mut watch, notices) = watch_with(&fs, &[], &["/m/dropbox"]);
    fs.link("/m/shortcut", "/m/docs");
    fs.link("/m/chain", "/m/shortcut");
    let events = [
        event("/m/shortcut", CREATED_LINK),
        event("/m/chain", CREATED_LINK),
    ];
    quiet(watch.events(&events));
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
    assert!(fs.exists("/m/shortcut") && fs.exists("/m/chain") && fs.exists("/m/dropbox"));
}

#[test]
fn names_hfs_reads_as_git_or_head_are_set_aside() {
    let fs = FakeFs::new();
    fs.dir("/m/docs");
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.g\u{200C}it");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    assert!(!fs.exists("/m/foo/.g\u{200C}it"));
    assert_moved(&notices, "foo/.g\u{200C}it");
    fs.file("/m/docs/\u{FEFF}He\u{200D}ad");
    let created = [event("/m/docs/\u{FEFF}He\u{200D}ad", CREATED_FILE)];
    found(&mut watch, &created);
    assert_set_aside(&notices, "docs/\u{FEFF}He\u{200D}ad", GitItem::BareRepoHead);
    // Names with ZWJ or ZWNJ that are not these stay.
    fs.file("/m/docs/\u{915}\u{94D}\u{200D}\u{937}.txt");
    fs.dir("/m/.gi\u{200B}t");
    quiet(watch.full_scan(Phase::Ended));
    none(&notices);
}

#[test]
fn the_agent_gets_a_plain_refusal_line() {
    let fs = FakeFs::new();
    let (mut watch, notices) = watch(&fs, &[]);
    fs.dir("/m/foo");
    fs.repo("/m/foo/.git");
    found(&mut watch, &[event("/m/foo", MOVED_IN)]);
    let notices = notices.taken();
    assert_eq!(
        notices[0].refusal().unwrap(),
        "Clauseo ended this session and set aside foo/.git: a matter can't hold a git \
         repository while a session runs."
    );
    let link = Denial::GitSetAside {
        path: PathBuf::from("foo"),
        what: GitItem::OutsideLink,
        outcome: GitOutcome::Stuck,
        session_ended: true,
    };
    assert!(
        link.refusal()
            .unwrap()
            .contains("a link inside a matter can't lead outside")
    );
    assert_eq!(Denial::Dropped { count: 1 }.refusal(), None);
}

#[test]
fn the_lawyer_notice_says_what_appeared_and_blames_no_one() {
    let notice = |path: &str, what, outcome, session_ended| {
        Denial::GitSetAside {
            path: PathBuf::from(path),
            what,
            outcome,
            session_ended,
        }
        .notice()
        .unwrap()
    };
    let moved = GitOutcome::Moved(PathBuf::from("/data/git-set-aside/x/item"));
    assert_eq!(
        notice("Clients/Acme/.git", GitItem::DotGit, moved.clone(), true),
        "A Git folder appeared in the matter at Clients/Acme/.git while an AI tool was \
         running. Clauseo stopped the session and moved it to /data/git-set-aside/x/item, \
         because Git folders can run programs when opened. If you put it there yourself, you \
         can move it back from there."
    );
    assert_eq!(
        notice("a", GitItem::BareRepo, moved, false),
        "A Git folder appeared in the matter at a during an AI session Clauseo could not \
         finish checking. Clauseo moved it to /data/git-set-aside/x/item, because Git folders \
         can run programs when opened. If you put it there yourself, you can move it back \
         from there."
    );
    let link = GitOutcome::LinkRemoved {
        target: PathBuf::from("/Users/x/secrets"),
    };
    assert_eq!(
        notice("a/link", GitItem::OutsideLink, link, true),
        "A link leading outside the matter appeared in the matter at a/link while an AI tool \
         was running. Clauseo stopped the session and removed the link, which led to \
         /Users/x/secrets, because Git folders can run programs when opened, and a link can \
         lead to one."
    );
    let renamed = GitOutcome::RenamedInPlace(PathBuf::from("/m/.git-set-aside-by-clauseo-1f"));
    let text = notice(".git", GitItem::DotGit, renamed, true);
    assert!(text.contains("renamed it to /m/.git-set"), "{text}");
    assert!(!text.contains("move it back"), "{text}");
    assert_eq!(
        notice("x/HEAD", GitItem::BareRepoHead, GitOutcome::Stuck, true),
        "A Git HEAD file appeared in the matter at x/HEAD while an AI tool was running. \
         Clauseo stopped the session, because Git folders can run programs when opened, but \
         could not move or rename it. Clauseo won't start an AI tool in this matter until \
         x/HEAD is moved out of it."
    );
    for text in [
        notice("a", GitItem::CommonDirPart, GitOutcome::Stuck, false),
        notice("a", GitItem::DotGit, GitOutcome::Stuck, true),
    ] {
        for blame in ["The AI tool", "tried to", "you made", "If you made"] {
            assert!(!text.contains(blame), "{text}");
        }
    }
    assert_eq!(Denial::Dropped { count: 1 }.notice(), None);
}

#[test]
fn every_path_a_notice_shows_has_its_controls_replaced_visibly() {
    let crafted = "a\u{202E}txt.exe\nb\u{2066}c\u{200F}\u{061C}\u{7}\u{2028}";
    let shown = r"a\u{202e}txt.exe\u{a}b\u{2066}c\u{200f}\u{61c}\u{7}\u{2028}";
    let denial = Denial::GitSetAside {
        path: PathBuf::from(crafted),
        what: GitItem::DotGit,
        outcome: GitOutcome::Moved(PathBuf::from(format!("/data/x/{crafted}"))),
        session_ended: true,
    };
    let notice = denial.notice().unwrap();
    assert_eq!(notice.matches(shown).count(), 2, "{notice}");
    let refusal = denial.refusal().unwrap();
    assert!(refusal.contains(shown), "{refusal}");
    for text in [notice, refusal] {
        assert!(
            !text.chars().any(|c| c.is_control()
                || matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')),
            "{text:?}"
        );
    }
    let link = Denial::GitSetAside {
        path: PathBuf::from("l"),
        what: GitItem::OutsideLink,
        outcome: GitOutcome::LinkRemoved {
            target: PathBuf::from("/x\u{202D}y"),
        },
        session_ended: false,
    };
    assert!(link.notice().unwrap().contains(r"/x\u{202d}y"));
}
