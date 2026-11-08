//! The Git trees' engine against the fake: a refresh rebuilds only on change, an account's old
//! trees stay while it runs, removal deletes them all, and a failure is recorded, never fatal.

use std::path::Path;

use super::fake::{ADMIN, FakeTrees, GIT_ROOT, key};
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
    assert_eq!(fake.count("rename"), 2, "only the first run renamed");
    assert_eq!(
        fake.count("walk"),
        1,
        "the first run listed the install once"
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

#[test]
fn an_accounts_previous_tree_stays_while_it_runs_and_goes_after() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2)], &group());
    fake.running.insert(account(1).sid);
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    let new = id_of(&mut fake);
    ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(
        fake.node(&tree(&account(1), &old)).is_some(),
        "account 1 runs"
    );
    assert!(fake.node(&tree(&account(1), &new)).is_some());
    assert!(
        fake.node(&tree(&account(2), &old)).is_none(),
        "account 2 doesn't"
    );
    // A file of the kept tree is still the old install's file, whatever the update did.
    let kept = fake
        .node(&tree(&account(1), &old).join(r"cmd\git.exe"))
        .cloned();
    let now = fake
        .node(Path::new(r"C:\Program Files\Git\cmd\git.exe"))
        .cloned();
    assert_ne!(kept, now);

    fake.running.clear();
    ensure(&mut fake, &[account(1), account(2)], &group());
    assert!(
        fake.node(&tree(&account(1), &old)).is_none(),
        "its run ended"
    );
    assert!(fake.node(&tree(&account(1), &new)).is_some());
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
fn removal_tries_the_record_when_the_folder_fails_and_reports_it() {
    let mut fake = FakeTrees::new();
    ensure(&mut fake, &[account(1)], &group());
    fake.fail.push("remove_tree ".to_owned());
    assert!(remove(&mut fake).is_err());
    assert!(fake.record.is_none());
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
    fake.fail.push("write_record".to_owned());
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
fn git_gone_later_records_unavailable_and_retires_idle_trees() {
    let mut fake = FakeTrees::new();
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
    assert!(fake.node(&tree(&account(1), &id)).is_none());
    assert!(
        fake.node(&tree(&account(2), &id)).is_some(),
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
fn a_file_with_several_names_in_the_install_is_linked_from_one_copy_of_each_name() {
    // NTFS holds a file to 1023 names: Git's built-ins linked to one git.exe, times every tree,
    // would pass it.
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
        2,
        "each name of the shared file, once"
    );
    let copy = Path::new(ADMIN).join("git").join("_source").join(&id);
    let copied = fake.node(&copy.join(r"mingw64\bin\git.exe")).cloned();
    assert_ne!(copied, Some(node.clone()));
    assert!(
        fake.node(&copy.join(r"usr\bin\bash.exe")).is_none(),
        "one name: not copied"
    );
    for account in &accounts {
        let tree = tree(account, &id);
        assert_eq!(
            fake.node(&tree.join(r"mingw64\bin\git.exe")).cloned(),
            copied
        );
        let bash = Path::new(GIT_ROOT).join(r"usr\bin\bash.exe");
        assert_eq!(fake.node(&tree.join(r"usr\bin\bash.exe")), fake.node(&bash));
        assert!(
            fake.node(&tree.join(r"mingw64\libexec\git-core\git-add.exe"))
                .is_some()
        );
    }
    assert_eq!(fake.node(&git), Some(&node), "the install is as it was");
}

#[test]
fn git_updated_during_a_build_fails_it_and_names_no_tree() {
    let mut fake = FakeTrees::new();
    fake.replace_on_link = Some(r"C:\Program Files\Git\cmd\git.exe".to_owned());
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
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    let from = fake.calls.len();
    ensure(&mut fake, &[account(1)], &group());
    let calls = &fake.calls[from..];
    let write = calls.iter().position(|c| c == "write_record -").unwrap();
    let old_tree = format!("remove_tree {}", key(&tree(&account(1), &old)));
    let gone = calls.iter().position(|c| *c == old_tree).unwrap();
    assert!(write < gone, "{calls:?}");
}

#[test]
fn a_record_that_cant_be_written_retires_nothing() {
    let mut fake = FakeTrees::new();
    let old = id_of(&mut fake);
    ensure(&mut fake, &[account(1)], &group());
    fake.new_file(r"C:\Program Files\Git\cmd\git.exe");
    fake.fail.push("write_record".to_owned());
    ensure(&mut fake, &[account(1)], &group());
    assert!(
        fake.node(&tree(&account(1), &old)).is_some(),
        "the record may still name it"
    );
}
