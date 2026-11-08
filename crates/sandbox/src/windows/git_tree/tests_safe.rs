//! The Git trees' engine against the fake: the plain delete and the trash, the install's safety
//! checks, the file-ID checks, the completion markers, `--adopt`'s check and the record's
//! per-value writes.

use std::path::{Path, PathBuf};

use super::fake::{ADMIN, FakeTrees, GIT_ROOT, key};
use super::tests::{account, group, id_of, tree};
use super::tests_more::update;
use super::{
    MARKER, Owner, Recorded, TRASH_DIR, Unavailable, check_adopted, ensure, marker_matches,
    marker_sddl, marker_text,
};

/// `%ProgramFiles%\Clauseo\Sandbox\git`.
fn git() -> PathBuf {
    Path::new(ADMIN).join("git")
}

/// The names in the trash.
fn trash(fake: &FakeTrees) -> Vec<String> {
    fake.below(&git().join(TRASH_DIR))
        .into_iter()
        .filter(|rel| !rel.contains('\\'))
        .collect()
}

/// The calls made since `from` whose line starts with `op`.
fn since<'a>(fake: &'a FakeTrees, from: usize, op: &str) -> Vec<&'a String> {
    fake.calls[from..]
        .iter()
        .filter(|line| line.starts_with(op))
        .collect()
}

#[test]
fn a_delete_that_meets_a_file_in_use_moves_it_to_the_trash_and_a_later_run_retries() {
    let mut fake = FakeTrees::new();
    let oldest = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    let held = key(&tree(&account(1), &oldest).join(r"usr\bin\bash.exe"));
    fake.in_use.insert(held);
    let from = fake.calls.len();
    let out = ensure(&mut fake, &[account(1)], &group());
    assert!(matches!(out[0].1, Recorded::Tree(_)));
    assert!(
        fake.node(&tree(&account(1), &oldest)).is_none(),
        "its path is free"
    );
    assert_eq!(trash(&fake).len(), 1, "the file in use waits in the trash");
    let trash_key = key(&git().join(TRASH_DIR));
    assert!(fake.in_use.iter().all(|at| at.starts_with(&trash_key)));
    // The delete never touched any security: the only folder given a DACL meanwhile is the
    // trash, and the trees' own chain, which every run secures again.
    let oldest_key = key(&tree(&account(1), &oldest));
    for line in since(&fake, from, "level ") {
        assert!(!line.contains(&oldest_key), "{line}");
    }
    assert!(fake.reboot.is_empty(), "a run leaves it for the next one");

    fake.in_use.clear();
    ensure(&mut fake, &[account(1)], &group());
    assert!(trash(&fake).is_empty(), "the next run emptied it");
}

#[test]
fn a_folder_that_can_be_moved_goes_to_the_trash_whole() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    fake.fail
        .push(format!("remove_tree {}", key(&tree(&account(1), &id))));
    // Another account's name: its folder is a former account's, deleted.
    ensure(&mut fake, &[account(2)], &group());
    assert!(fake.node(&git().join("ClauseoSb01")).is_none());
}

#[test]
fn an_install_others_can_change_anywhere_inside_is_source_unsafe() {
    for inside in [r"usr\bin", r"etc\fstab", r"mingw64"] {
        let mut fake = FakeTrees::new();
        fake.open_to_users(&Path::new(GIT_ROOT).join(inside));
        let out = ensure(&mut fake, &[account(1)], &group());
        assert_eq!(
            out[0].1,
            Recorded::Unavailable(Unavailable::SourceUnsafe),
            "{inside}"
        );
        assert_eq!(fake.count("copy_file"), 0);
        assert_eq!(fake.count("hard_link"), 0);
    }
    assert_eq!(
        Unavailable::from_code("source-unsafe"),
        Some(Unavailable::SourceUnsafe)
    );
}

#[test]
fn a_file_replaced_after_the_walk_refuses_the_copy() {
    let mut fake = FakeTrees::new();
    // The last file copied, swapped as the first is copied: its ID isn't the walk's.
    fake.replace_on_copy = Some(r"C:\Program Files\Git\usr\bin\sh.exe".to_owned());
    let id = id_of(&mut fake);
    let out = ensure(&mut fake, &[account(1)], &group());
    assert_eq!(out[0].1, Recorded::Unavailable(Unavailable::BuildFailed));
    let sources = git().join("_source");
    assert_eq!(
        fake.below(&sources),
        Vec::<String>::new(),
        "no copy, finished or not"
    );
    assert!(fake.node(&tree(&account(1), &id)).is_none());
}

#[test]
fn a_copy_file_replaced_before_it_is_linked_refuses_the_tree() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let copy = git().join("_source").join(&id);
    // Account 2's build lists the copy, then the file changes before its link.
    fake.replace_on_copy = Some(key(&copy.join(r"usr\bin\sh.exe")));
    let out = ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(
        matches!(out[0].1, Recorded::Tree(_)),
        "account 1's tree is kept"
    );
    assert_eq!(out[1].1, Recorded::Unavailable(Unavailable::BuildFailed));
    assert_eq!(fake.below(&git().join("ClauseoSb02")), Vec::<String>::new());
}

#[test]
fn a_tree_without_its_marker_is_built_again() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let tree = tree(&account(1), &id);
    let marker = key(&tree.join(MARKER));
    fake.nodes.remove(&marker);
    fake.texts.remove(&marker);
    let renames = fake.count("rename");
    ensure(&mut fake, &[account(1)], &group());
    assert_eq!(
        fake.count("rename"),
        renames + 1,
        "the tree, again; not the copy"
    );
    assert_eq!(
        fake.texts.get(&marker).map(String::as_str),
        Some(marker_text(&id, Some(&account(1).sid)).as_str())
    );
}

#[test]
fn a_recreated_account_with_a_new_sid_gets_a_new_tree() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let mut again = account(1);
    again.sid = account(9).sid;
    let from = fake.calls.len();
    let out = ensure(&mut fake, &[again.clone()], &group());
    assert_eq!(out[0].1, Recorded::Tree(tree(&again, &id)));
    assert_eq!(since(&fake, from, "hard_link ").len(), 6, "linked again");
    let marker = &fake.texts[&key(&tree(&again, &id).join(MARKER))];
    assert!(marker.ends_with(&format!("{}\n", again.sid)));
}

#[test]
fn a_tree_a_folder_or_git_not_owned_by_system_is_built_again() {
    let id = id_of(&mut FakeTrees::new());
    let tree = tree(&account(1), &id);
    for (level, links) in [
        (tree.clone(), 6),
        (tree.parent().unwrap().to_path_buf(), 6),
        (git().join("_source").join(&id), 0),
        (git(), 6),
    ] {
        let mut fake = FakeTrees::new();
        ensure(&mut fake, &[account(1)], &group());
        fake.own(&level, Owner::Administrators);
        let from = fake.calls.len();
        let out = ensure(&mut fake, &[account(1)], &group());
        assert_eq!(
            out[0].1,
            Recorded::Tree(tree.clone()),
            "{}",
            level.display()
        );
        assert_eq!(
            since(&fake, from, "hard_link ").len(),
            links,
            "{}",
            level.display()
        );
        // The copy is checked every run, and made again when the next build needs it.
        assert!(
            fake.node(&level).is_none() || fake.meta_of(&level).owner == Owner::System,
            "{}",
            level.display()
        );
    }
}

#[test]
fn a_copy_without_its_marker_is_copied_again() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let copy = git().join("_source").join(&id);
    fake.texts
        .insert(key(&copy.join(MARKER)), "other-id\n".to_owned());
    let tree_marker = key(&tree(&account(1), &id).join(MARKER));
    fake.nodes.remove(&tree_marker);
    fake.texts.remove(&tree_marker);
    let from = fake.calls.len();
    ensure(&mut fake, &[account(1)], &group());
    assert_eq!(since(&fake, from, "copy_file ").len(), 6);
    assert_eq!(
        fake.texts.get(&key(&copy.join(MARKER))).map(String::as_str),
        Some(format!("{id}\n").as_str())
    );
}

#[test]
fn the_markers_name_the_tree_and_the_account_and_any_user_may_read_them() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let sid = account(1).sid;
    let marker = key(&tree(&account(1), &id).join(MARKER));
    assert_eq!(fake.texts[&marker], format!("{id}\n{sid}\n"));
    assert_eq!(fake.marker_sddls[&marker], marker_sddl(Some(&sid)));
    assert_eq!(
        marker_sddl(Some(&sid)),
        format!("O:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x1200a9;;;{sid})(A;;0x120089;;;AU)")
    );
    let copy = key(&git().join("_source").join(&id).join(MARKER));
    assert_eq!(fake.marker_sddls[&copy], marker_sddl(None));
    assert!(marker_sddl(None).ends_with("(A;;0x120089;;;AU)"));
    // The reader's rules: CRLF, and the id without case.
    let wanted = marker_text("2.47.1.1-ABCDEF012345", Some(&sid));
    assert!(marker_matches(
        &format!("2.47.1.1-abcdef012345\r\n{sid}\r\n"),
        &wanted
    ));
    assert!(!marker_matches(
        &format!("2.47.1.1-abcdef012345\n{}\n", account(2).sid),
        &wanted
    ));
    assert!(!marker_matches("2.47.1.1-abcdef012345\n", &wanted));
}

#[test]
fn a_swapped_dll_anywhere_in_the_install_names_a_new_tree() {
    let mut fake = FakeTrees::new();
    let before = id_of(&mut fake);
    fake.new_file(r"C:\Program Files\Git\usr\bin\sh.exe");
    assert_ne!(before, id_of(&mut fake));
    let again = id_of(&mut fake);
    fake.folders(r"C:\Program Files\Git\usr\lib");
    assert_ne!(again, id_of(&mut fake), "a new folder changes the listing");
}

#[test]
fn the_record_is_written_value_by_value_and_never_deleted_first() {
    let mut fake = FakeTrees::new();
    ensure(&mut fake, &[account(1), account(2)], &group());
    let from = fake.calls.len();
    ensure(&mut fake, &[account(1)], &group());
    assert_eq!(since(&fake, from, "delete_record "), Vec::<&String>::new());
    let deletes = since(&fake, from, "delete_record_value ");
    assert_eq!(
        deletes,
        [&format!("delete_record_value {}", account(2).sid)]
    );
    let set = fake.calls[from..]
        .iter()
        .position(|c| c.starts_with("set_record_value "))
        .unwrap();
    let deleted = fake.calls[from..]
        .iter()
        .position(|c| c.starts_with("delete_record_value "))
        .unwrap();
    assert!(
        set < deleted,
        "the new value is there before the old one goes"
    );
    assert_eq!(fake.record.clone().unwrap().len(), 1);
}

#[test]
fn adopt_deletes_each_level_not_owned_by_system_or_administrators() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2), account(3)], &group());
    fake.own(&git().join("ClauseoSb01"), Owner::Other);
    fake.own(&tree(&account(2), &id), Owner::Other);
    fake.own(&tree(&account(3), &id), Owner::Administrators);
    check_adopted(&mut fake, &git());
    assert!(fake.node(&git().join("ClauseoSb01")).is_none());
    assert!(fake.node(&tree(&account(2), &id)).is_none());
    assert!(
        fake.node(&git().join("ClauseoSb02")).is_some(),
        "only the level that failed"
    );
    assert!(
        fake.node(&tree(&account(3), &id)).is_some(),
        "Administrators' is kept"
    );
    assert!(fake.node(&git().join("_source").join(&id)).is_some());
    assert_eq!(fake.count("level"), 9, "the check sets no security");
    // The next run builds what went.
    let out = ensure(&mut fake, &[account(1), account(2), account(3)], &group());
    assert!(out.iter().all(|(_, r)| matches!(r, Recorded::Tree(_))));
}

#[test]
fn adopt_deletes_a_link_or_a_level_others_can_change_and_a_foreign_git_folder() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2)], &group());
    let link = git().join("ClauseoSb01").join("junction");
    fake.nodes.insert(key(&link), super::fake::Node::Link);
    fake.open_to_users(&tree(&account(2), &id));
    check_adopted(&mut fake, &git());
    assert!(fake.node(&link).is_none());
    assert!(fake.node(&tree(&account(2), &id)).is_none());
    assert!(fake.node(&tree(&account(1), &id)).is_some());

    fake.own(&git(), Owner::Other);
    check_adopted(&mut fake, &git());
    assert!(fake.node(&git()).is_none(), "nothing in it is trusted");
    check_adopted(&mut fake, &git());
}

#[test]
fn a_finished_tree_has_its_marker_and_a_half_built_one_none() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let done = tree(&account(1), &id).join(MARKER);
    assert!(fake.texts.contains_key(&key(&done)));
    let copy = git().join("_source").join(&id).join(MARKER);
    assert_eq!(fake.texts[&key(&copy)], format!("{id}\n"));
    // Account 2's build dies at a link: no marker is written, and nothing is left behind.
    let temp = key(&tree(&account(2), &id));
    fake.fail.push(format!(r"-> {temp}.tmp\usr\bin\sh.exe"));
    let out = ensure(&mut fake, &[account(1), account(2)], &group());
    assert_eq!(out[1].1, Recorded::Unavailable(Unavailable::BuildFailed));
    assert!(fake.texts.keys().all(|at| !at.contains("clauseosb02")));
    assert!(
        !fake
            .calls
            .iter()
            .any(|c| c.starts_with("write_marker") && c.contains("clauseosb02")),
        "the marker is written only once every file is in"
    );
    assert_eq!(fake.below(&git().join("ClauseoSb02")), Vec::<String>::new());
}

#[test]
fn a_tree_folder_a_run_holds_open_never_fails_the_run_and_goes_later() {
    // The outer runner holds a tree's folder open without delete sharing for a whole run, and
    // any signed-in user may hold one open through the Authenticated Users entry.
    let mut fake = FakeTrees::new();
    let oldest = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    let held = tree(&account(1), &oldest);
    fake.in_use.insert(key(&held));
    let out = ensure(&mut fake, &[account(1)], &group());
    assert!(matches!(out[0].1, Recorded::Tree(_)), "{out:?}");
    assert!(fake.logs.iter().all(|(error, _)| !error), "{:?}", fake.logs);
    assert!(fake.node(&held).is_some(), "the held folder stays");
    assert!(fake.below(&held).is_empty(), "what is in it went");

    fake.in_use.clear();
    ensure(&mut fake, &[account(1)], &group());
    assert!(fake.node(&held).is_none(), "a later run deleted it");
}

#[test]
fn nothing_below_git_is_ever_taken_back_locked_down_or_given_a_propagating_dacl() {
    // Those set a DACL that propagates onto the trees' files, which are hard links: the High
    // of stage F. Every delete is a plain one, and security is set on a folder alone.
    let sources = [
        ("os.rs", include_str!("os.rs")),
        ("os/delete.rs", include_str!("os/delete.rs")),
        ("os/files.rs", include_str!("os/files.rs")),
        ("os/level.rs", include_str!("os/level.rs")),
        ("os/nt.rs", include_str!("os/nt.rs")),
        ("os/put.rs", include_str!("os/put.rs")),
        ("os/walk.rs", include_str!("os/walk.rs")),
        ("build.rs", include_str!("build.rs")),
        ("discard.rs", include_str!("discard.rs")),
        ("retire.rs", include_str!("retire.rs")),
    ];
    let banned = [
        "take_back(",
        "remove_taking_back",
        "lock_tree(",
        "NtfsLockDown",
        "SetSecurityInfo(",
        "SetNamedSecurityInfo",
        "remove_dir_all",
    ];
    for (name, text) in sources {
        for word in banned {
            assert!(!text.contains(word), "{name} names {word}");
        }
    }
}
