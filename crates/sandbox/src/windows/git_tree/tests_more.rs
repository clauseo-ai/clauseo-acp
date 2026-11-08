//! The Git trees' engine against the fake: a refresh rebuilds only on change, an account's old
//! trees stay while it runs, removal deletes them all, and a failure is recorded, never fatal.

use std::path::Path;

use super::fake::{ADMIN, FakeTrees, GIT_FILES, GIT_ROOT, Node, key};
use super::tests::{account, group, id_of, tree};
use super::{Recorded, UNAVAILABLE, Unavailable, build, ensure, remove};

#[test]
fn a_refresh_with_the_same_git_builds_nothing() {
    let mut fake = FakeTrees::new();
    let accounts = [account(1), account(2)];
    let first = ensure(&mut fake, &accounts, &group());
    let links = fake.count("hard_link");
    let second = ensure(&mut fake, &accounts, &group());
    assert_eq!(first, second);
    assert_eq!(fake.count("hard_link"), links, "no file linked again");
    assert_eq!(
        fake.count("rename"),
        3,
        "only the first run renamed: the copy and two trees"
    );
    assert_eq!(fake.count("copy_file"), GIT_FILES.len(), "copied once");
    assert_eq!(
        fake.count("walk"),
        3,
        "each run lists the install; the first, its copy too"
    );
}

#[test]
fn a_refresh_after_a_git_update_builds_a_new_tree_and_records_it() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    fake.new_file(r"C:\Program Files\Git\usr\bin\msys-2.0.dll");
    let new = id_of(&mut fake);
    assert_ne!(old, new);
    let out = ensure(&mut fake, &[account(1)], &group());
    let current = tree(&account(1), &new);
    assert_eq!(out[0].1, Recorded::Tree(current.clone()));
    assert!(fake.node(&current).is_some());
    let record = fake.record.clone().unwrap();
    assert_eq!(record[0].1, current.display().to_string());
}

/// A Git update: the install's `git.exe` replaced.
pub(super) fn update(fake: &mut FakeTrees) {
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
}

#[test]
fn the_current_and_previous_trees_stay_and_older_ones_go_once_the_account_is_idle() {
    let mut fake = FakeTrees::new();
    let accounts = [account(1), account(2)];
    let oldest = id_of(&mut fake);
    ensure(&mut fake, &accounts, &group());
    update(&mut fake);
    let previous = id_of(&mut fake);
    ensure(&mut fake, &accounts, &group());
    fake.running.insert(account(1).sid);
    update(&mut fake);
    let current = id_of(&mut fake);
    ensure(&mut fake, &accounts, &group());
    for id in [&oldest, &previous, &current] {
        assert!(
            fake.node(&tree(&account(1), id)).is_some(),
            "account 1 runs"
        );
    }
    assert!(
        fake.node(&tree(&account(2), &oldest)).is_none(),
        "account 2 doesn't"
    );
    for id in [&previous, &current] {
        assert!(fake.node(&tree(&account(2), id)).is_some(), "kept: {id}");
    }
    // A file of a kept tree is still the old copy's, whatever the update did.
    let kept = fake
        .node(&tree(&account(1), &oldest).join(r"cmd\git.exe"))
        .cloned();
    let now = fake
        .node(Path::new(r"C:\Program Files\Git\cmd\git.exe"))
        .cloned();
    assert_ne!(kept, now);

    fake.running.clear();
    ensure(&mut fake, &accounts, &group());
    assert!(
        fake.node(&tree(&account(1), &oldest)).is_none(),
        "its run ended"
    );
    for id in [&previous, &current] {
        assert!(fake.node(&tree(&account(1), id)).is_some(), "kept: {id}");
    }
}

#[test]
fn every_older_tree_stays_while_the_account_runs() {
    let mut fake = FakeTrees::new();
    fake.running.insert(account(1).sid);
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(id_of(&mut fake));
        ensure(&mut fake, &[account(1)], &group());
        fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    }
    for id in &ids {
        assert!(fake.node(&tree(&account(1), id)).is_some(), "{id}");
    }
}

#[test]
fn a_process_list_that_cant_be_read_keeps_the_previous_tree() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    fake.processes_unreadable = true;
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    ensure(&mut fake, &[account(1)], &group());
    assert!(fake.node(&tree(&account(1), &old)).is_some());
}

#[test]
fn a_killed_runs_temporary_tree_is_deleted_even_while_the_account_runs() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    fake.folders(&format!(r"{ADMIN}\git\ClauseoSb01\{id}.tmp\usr"));
    fake.folders(&format!(r"{ADMIN}\git\ClauseoSb01\old-000000000000.tmp"));
    fake.running.insert(account(1).sid);
    ensure(&mut fake, &[account(1)], &group());
    let folder = Path::new(ADMIN).join("git").join("ClauseoSb01");
    let names: Vec<String> = fake
        .below(&folder)
        .into_iter()
        .filter(|rel| !rel.contains('\\'))
        .collect();
    assert_eq!(names, [id.to_lowercase()]);
}

#[test]
fn a_former_accounts_folder_is_deleted() {
    let mut fake = FakeTrees::new();
    ensure(&mut fake, &[account(1), account(2)], &group());
    ensure(&mut fake, &[account(1)], &group());
    let git = Path::new(ADMIN).join("git");
    assert!(fake.node(&git.join("ClauseoSb02")).is_none());
    assert!(fake.node(&git.join("ClauseoSb01")).is_some());
    assert_eq!(fake.record.clone().unwrap().len(), 1);
}

#[test]
fn removal_deletes_every_tree_and_the_record() {
    let mut fake = FakeTrees::new();
    fake.install(r"D:\Git");
    fake.registry = Some(r"D:\Git".to_owned());
    fake.running.insert(account(1).sid);
    ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(fake.record.is_some());
    remove(&mut fake).unwrap();
    assert!(fake.node(&Path::new(ADMIN).join("git")).is_none());
    assert_eq!(
        fake.below(&Path::new(ADMIN).join("git")),
        Vec::<String>::new()
    );
    assert!(fake.record.is_none());
    assert!(
        fake.node(Path::new(ADMIN)).is_some(),
        "the admin folder stays"
    );
}

#[test]
fn removal_with_a_file_in_use_succeeds_and_schedules_the_rest_for_the_restart() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    let bash = key(&tree(&account(1), &id).join(r"usr\bin\bash.exe"));
    fake.in_use.insert(bash);
    remove(&mut fake).expect("a file in use never fails the removal");
    assert!(fake.record.is_none());
    let git = key(&Path::new(ADMIN).join("git"));
    assert_eq!(fake.reboot, std::slice::from_ref(&git));
    let left: Vec<&String> = fake.in_use.iter().collect();
    assert_eq!(left.len(), 1);
    assert!(
        left[0].starts_with(&format!(r"{git}\_trash\")),
        "the file in use waits in the trash: {left:?}"
    );
    assert_eq!(fake.below(&tree(&account(1), &id)), Vec::<String>::new());
    assert!(
        fake.logs
            .iter()
            .any(|(error, line)| !error && line.contains("next restart"))
    );
}

#[test]
fn removal_reports_a_record_that_cant_be_deleted() {
    let mut fake = FakeTrees::new();
    ensure(&mut fake, &[account(1)], &group());
    fake.fail.push("delete_record".to_owned());
    assert!(remove(&mut fake).is_err());
    assert!(fake.node(&Path::new(ADMIN).join("git")).is_none());
}

#[test]
fn one_accounts_failure_is_recorded_and_the_others_go_on() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    let other = r"-> c:\program files\clauseo\sandbox\git\clauseosb02\";
    fake.fail.push(other.to_owned());
    let out = ensure(&mut fake, &[account(1), account(2), account(3)], &group());
    assert_eq!(out[0].1, Recorded::Tree(tree(&account(1), &id)));
    assert_eq!(out[1].1, Recorded::Unavailable(Unavailable::BuildFailed));
    assert_eq!(out[2].1, Recorded::Tree(tree(&account(3), &id)));
    let record = fake.record.clone().unwrap();
    assert_eq!(record[1].1, "Git Bash unavailable: build-failed");
    let errors: Vec<&String> = fake
        .logs
        .iter()
        .filter(|(e, _)| *e)
        .map(|(_, l)| l)
        .collect();
    assert_eq!(errors.len(), 1);
    for line in errors {
        assert!(
            !line.contains('\\') && !line.contains("ClauseoSb"),
            "no path or name: {line}"
        );
    }
}

#[test]
fn a_folder_that_fails_its_check_is_folder_refused() {
    let mut fake = FakeTrees::new();
    fake.fail
        .push(r"level c:\program files\clauseo\sandbox\git\clauseosb01".to_owned());
    let out = ensure(&mut fake, &[account(1), account(2)], &group());
    assert_eq!(out[0].1, Recorded::Unavailable(Unavailable::FolderRefused));
    assert!(matches!(out[1].1, Recorded::Tree(_)));
}

#[test]
fn an_admin_folder_that_fails_its_check_leaves_every_account_unavailable() {
    let mut fake = FakeTrees::new();
    fake.fail.push("admin_dir".to_owned());
    let out = ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(
        out.iter()
            .all(|(_, r)| *r == Recorded::Unavailable(Unavailable::FolderRefused))
    );
    assert_eq!(fake.count("hard_link"), 0);
    assert_eq!(fake.record.clone().unwrap().len(), 2);
}

#[test]
fn a_record_that_cant_be_written_is_logged_and_the_trees_stay() {
    let mut fake = FakeTrees::new();
    let id = id_of(&mut fake);
    fake.fail.push("set_record_value".to_owned());
    let out = ensure(&mut fake, &[account(1)], &group());
    assert_eq!(out[0].1, Recorded::Tree(tree(&account(1), &id)));
    assert!(
        fake.logs
            .iter()
            .any(|(e, l)| *e && l.contains("couldn't be recorded"))
    );
}

#[test]
fn a_name_that_cant_be_a_folder_is_bad_account() {
    let mut fake = FakeTrees::new();
    let mut odd = account(1);
    for name in [
        "",
        "..",
        r"a\b",
        "_source",
        "ClauseoSb01:x",
        "abcdefghijklmnopqrstu",
    ] {
        odd.name = name.to_owned();
        let out = ensure(&mut fake, &[odd.clone()], &group());
        assert_eq!(
            out[0].1,
            Recorded::Unavailable(Unavailable::BadAccount),
            "{name:?}"
        );
    }
    assert_eq!(fake.count("hard_link"), 0);
}

#[test]
fn git_gone_later_records_unavailable_and_keeps_only_the_newest_idle_tree() {
    let mut fake = FakeTrees::new();
    let older = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2)], &group());
    update(&mut fake);
    let id = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2)], &group());
    fake.running.insert(account(2).sid);
    fake.registry = None;
    fake.nodes
        .retain(|path, _| !path.starts_with(r"c:\program files\git"));
    let out = ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(
        out.iter()
            .all(|(_, r)| *r == Recorded::Unavailable(Unavailable::NoGit))
    );
    assert!(fake.node(&tree(&account(1), &older)).is_none());
    assert!(
        fake.node(&tree(&account(1), &id)).is_some(),
        "the previous tree"
    );
    assert!(
        fake.node(&tree(&account(2), &older)).is_some(),
        "account 2 runs"
    );
}

#[test]
fn a_recorded_value_reads_back_as_written() {
    let path = tree(&account(1), "2.47.1.1-0123456789ab");
    for recorded in [
        Recorded::Tree(path),
        Recorded::Unavailable(Unavailable::NoGit),
        Recorded::Unavailable(Unavailable::BadAccount),
    ] {
        assert_eq!(Recorded::parse(&recorded.text()), Some(recorded));
    }
    assert_eq!(Recorded::parse(&format!("{UNAVAILABLE}what")), None);
    assert_eq!(Recorded::parse(""), None);
    assert_eq!(
        build::unavailable_line(Unavailable::PerUserGit),
        "a sandbox account's Git tree isn't available (Git Bash unavailable: per-user-git)"
    );
    assert_eq!(key(Path::new(r"C:\A")), r"c:\a");
}

#[test]
fn the_trees_link_only_the_copy_and_never_the_install() {
    // NTFS holds a file to 1023 names, and the copy's files are ours: one name each, plus one
    // per tree. The install's built-ins linked to one git.exe are copied name by name.
    let mut fake = FakeTrees::new();
    let git = Path::new(GIT_ROOT).join(r"mingw64\bin\git.exe");
    let node = fake.node(&git).cloned().unwrap();
    let builtin = Path::new(GIT_ROOT).join(r"mingw64\libexec\git-core\git-add.exe");
    fake.folders(r"C:\Program Files\Git\mingw64\libexec\git-core");
    fake.nodes.insert(key(&builtin), node.clone());
    let id = id_of(&mut fake);
    let accounts = [account(1), account(2)];
    ensure(&mut fake, &accounts, &group());
    assert_eq!(
        fake.count("copy_file"),
        GIT_FILES.len() + 1,
        "each name once"
    );
    let install: Vec<Node> = fake
        .below(Path::new(GIT_ROOT))
        .iter()
        .filter_map(|rel| fake.node(&Path::new(GIT_ROOT).join(rel)).cloned())
        .filter(|node| matches!(node, Node::File(_)))
        .collect();
    for account in &accounts {
        let tree = tree(account, &id);
        for rel in fake.below(&tree) {
            let linked = fake.node(&tree.join(&rel)).cloned().unwrap();
            assert!(!install.contains(&linked), "{rel} isn't the install's file");
        }
        assert!(
            fake.node(&tree.join(r"mingw64\libexec\git-core\git-add.exe"))
                .is_some()
        );
    }
    let links: Vec<&String> = fake
        .calls
        .iter()
        .filter(|c| c.starts_with("hard_link "))
        .collect();
    assert!(
        links.iter().all(|c| c.contains(r"\git\_source\")),
        "every link names the copy: {links:?}"
    );
    assert_eq!(fake.node(&git), Some(&node), "the install is as it was");
}

#[test]
fn git_updated_during_a_build_fails_it_and_names_no_tree() {
    let mut fake = FakeTrees::new();
    fake.replace_on_copy = Some(r"C:\Program Files\Git\cmd\git.exe".to_owned());
    let out = ensure(&mut fake, &[account(1)], &group());
    assert_eq!(out[0].1, Recorded::Unavailable(Unavailable::BuildFailed));
    assert_eq!(fake.count("rename"), 0);
    let folder = Path::new(ADMIN).join("git").join("ClauseoSb01");
    assert_eq!(fake.below(&folder), Vec::<String>::new());
    // The next run names the new Git and builds it.
    assert!(matches!(
        ensure(&mut fake, &[account(1)], &group())[0].1,
        Recorded::Tree(_)
    ));
}

#[test]
fn removal_needs_no_admin_copy() {
    let mut fake = FakeTrees::new();
    ensure(&mut fake, &[account(1)], &group());
    fake.fail.push("admin_dir".to_owned());
    remove(&mut fake).unwrap();
    assert!(fake.node(&Path::new(ADMIN).join("git")).is_none());
    assert!(fake.record.is_none());
    // Nothing there at all is done too.
    remove(&mut fake).unwrap();
}

#[test]
fn the_record_names_the_new_tree_before_an_old_one_goes() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    let from = fake.calls.len();
    ensure(&mut fake, &[account(1)], &group());
    let calls = &fake.calls[from..];
    let write = calls
        .iter()
        .position(|c| c.starts_with("set_record_value "))
        .unwrap();
    let old_tree = format!("remove_tree {}", key(&tree(&account(1), &old)));
    let gone = calls.iter().position(|c| *c == old_tree).unwrap();
    assert!(write < gone, "{calls:?}");
}

#[test]
fn a_record_that_cant_be_written_retires_nothing() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    update(&mut fake);
    fake.fail.push("set_record_value".to_owned());
    ensure(&mut fake, &[account(1)], &group());
    assert!(
        fake.node(&tree(&account(1), &old)).is_some(),
        "the record may still name it"
    );
}
