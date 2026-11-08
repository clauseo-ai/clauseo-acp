//! The Git Bash stub against the fake: the set-up places `git\_off\bash.exe`, the refresh and
//! retirement keep it, `--adopt` checks it, the removal deletes it, and a failure to place it
//! never fails the set-up; only a runner signed with the updater key is ever copied.

use std::path::{Path, PathBuf};

use super::fake::{ADMIN, FakeTrees, Node, key};
use super::tests::{account, group};
use super::tests_more::update;
use super::{
    OFF_DIR, Owner, Recorded, STUB_NAME, Unavailable, check_adopted, ensure, ensure_with, off_sddl,
    remove,
};
use crate::windows::refresh::test_signer::{TestKey, comment};

/// The install folder's runner.
const RUNNER: &str = r"C:\Users\lawyer\App\clauseo-sandbox-run.exe";

/// `%ProgramFiles%\Clauseo\Sandbox\git`.
fn git() -> PathBuf {
    Path::new(ADMIN).join("git")
}

/// `git\_off\bash.exe`.
fn stub() -> PathBuf {
    git().join(OFF_DIR).join(STUB_NAME)
}

/// The runner's bytes.
const IMAGE: &[u8] = b"MZ the runner";

/// A machine with the runner in the lawyer's install folder, signed with the updater key.
fn with_runner() -> FakeTrees {
    let key = TestKey::new(0x1234);
    let mut fake = FakeTrees::new();
    fake.folders(r"C:\Users\lawyer\App");
    fake.new_file(RUNNER);
    fake.new_file(&format!("{RUNNER}.sig"));
    fake.key = key.public();
    sign(&mut fake, &key, "clauseo-sandbox-run.exe", IMAGE);
    fake
}

/// `image` as the runner, with a `.sig` by `key` naming `file`.
fn sign(fake: &mut FakeTrees, key: &TestKey, file: &str, image: &[u8]) {
    let sig = key.sign(image, &comment(file, "1.2.3"));
    fake.bytes.insert(key_of(RUNNER), image.to_vec());
    fake.bytes.insert(key_of(&format!("{RUNNER}.sig")), sig);
}

fn key_of(path: &str) -> String {
    key(Path::new(path))
}

/// A set-up's run: the trees, and the stub from [`RUNNER`].
fn set_up(fake: &mut FakeTrees) -> Vec<(super::Sid, Recorded)> {
    ensure_with(fake, &[account(1)], &group(), Some(Path::new(RUNNER)))
}

#[test]
fn the_set_up_places_a_copy_of_the_runner_as_the_stub() {
    let mut fake = with_runner();
    let out = set_up(&mut fake);
    assert!(matches!(out[0].1, Recorded::Tree(_)));
    let (Some(Node::File(copy)), Some(Node::File(runner))) =
        (fake.node(&stub()), fake.node(Path::new(RUNNER)))
    else {
        panic!(
            "the stub and the runner are files: {:?}",
            fake.below(&git())
        );
    };
    assert_ne!(copy, runner, "a copy, not a link of the lawyer's file");
    assert_eq!(
        fake.node(&git().join(OFF_DIR)),
        Some(&Node::Folder(Some(off_sddl(&group()))))
    );
    assert!(off_sddl(&group()).ends_with("(A;OIIO;0x1200a9;;;BU)(A;;0x120080;;;AU)"));
    assert_eq!(fake.meta_of(&stub()).owner, Owner::System);
    assert_eq!(fake.below(&git().join(OFF_DIR)), [STUB_NAME.to_lowercase()]);
    assert!(fake.logs.iter().all(|(error, _)| !error), "{:?}", fake.logs);
}

#[test]
fn a_second_set_up_replaces_the_stub_and_deletes_what_else_is_in_its_folder() {
    let mut fake = with_runner();
    set_up(&mut fake);
    let first = fake.node(&stub()).cloned();
    fake.new_file(&key(&git()
        .join(OFF_DIR)
        .join("bash.exe.0000000000000001.tmp")));
    set_up(&mut fake);
    assert_ne!(
        fake.node(&stub()).cloned(),
        first,
        "the current runner's copy"
    );
    let below = fake.below(&git().join(OFF_DIR));
    assert_eq!(below.len(), 2, "{below:?}");
    assert!(below.contains(&STUB_NAME.to_lowercase()));
    assert!(
        below.iter().any(|name| Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("old"))),
        "the old stub set aside, as a running one must be: {below:?}"
    );
    fake.in_use.insert(key(&stub()));
    set_up(&mut fake);
    assert_eq!(
        fake.bytes.get(&key(&stub())).map(Vec::as_slice),
        Some(IMAGE),
        "placed though the old one runs"
    );
}

#[test]
fn only_a_runner_signed_with_the_updater_key_is_copied() {
    let other = TestKey::new(0x9999);
    let plants: [fn(&mut FakeTrees, &TestKey); 5] = [
        |f, other| sign(f, other, "clauseo-sandbox-run.exe", IMAGE),
        |f, _| {
            let key = TestKey::new(0x1234);
            f.key = key.public();
            sign(f, &key, "clauseo-sandbox-setup.exe", IMAGE);
        },
        |f, _| {
            f.bytes.insert(key_of(RUNNER), b"MZ planted".to_vec());
        },
        |f, _| {
            f.bytes
                .insert(key_of(&format!("{RUNNER}.sig")), b"garbage".to_vec());
        },
        |f, _| {
            f.nodes.remove(&key_of(&format!("{RUNNER}.sig")));
        },
    ];
    for (n, plant) in plants.iter().enumerate() {
        let mut fake = with_runner();
        plant(&mut fake, &other);
        let out = set_up(&mut fake);
        assert!(
            matches!(out[0].1, Recorded::Tree(_)),
            "{n}: the set-up goes on"
        );
        assert!(fake.node(&stub()).is_none(), "{n}: never copied");
        assert_eq!(fake.count("place_file"), 0, "{n}");
    }
    let mut fake = with_runner();
    set_up(&mut fake);
    assert_eq!(
        fake.bytes.get(&key(&stub())).map(Vec::as_slice),
        Some(IMAGE)
    );
}

#[test]
fn the_stub_is_placed_with_no_git_too() {
    let mut fake = with_runner();
    fake.registry = None;
    fake.nodes
        .retain(|path, _| !path.starts_with(r"c:\program files\git"));
    let out = set_up(&mut fake);
    assert_eq!(out[0].1, Recorded::Unavailable(Unavailable::NoGit));
    assert!(matches!(fake.node(&stub()), Some(Node::File(_))));
}

#[test]
fn the_refresh_and_retirement_keep_the_stub() {
    let mut fake = with_runner();
    set_up(&mut fake);
    let placed = fake.node(&stub()).cloned();
    for _ in 0..3 {
        update(&mut fake);
        ensure(&mut fake, &[account(1)], &group());
    }
    assert_eq!(fake.node(&stub()).cloned(), placed);
    assert_eq!(fake.count("place_file"), 1, "the refresh copies nothing");
}

#[test]
fn a_refresh_with_no_stub_makes_none() {
    let mut fake = with_runner();
    ensure(&mut fake, &[account(1)], &group());
    assert!(fake.node(&git().join(OFF_DIR)).is_none());
    assert_eq!(fake.count("place_file"), 0);
}

#[test]
fn adopt_keeps_a_stub_administrators_own_and_deletes_another() {
    for (owner, kept) in [
        (Owner::System, true),
        (Owner::Administrators, true),
        (Owner::Other, false),
    ] {
        let mut fake = with_runner();
        set_up(&mut fake);
        fake.own(&stub(), owner);
        check_adopted(&mut fake, &git());
        assert_eq!(fake.node(&stub()).is_some(), kept, "{owner:?}");
        assert!(fake.node(&git().join(OFF_DIR)).is_some(), "{owner:?}");
    }
    let mut fake = with_runner();
    set_up(&mut fake);
    fake.own(&git().join(OFF_DIR), Owner::Other);
    check_adopted(&mut fake, &git());
    assert!(
        fake.node(&git().join(OFF_DIR)).is_none(),
        "its folder goes whole"
    );
}

#[test]
fn the_removal_deletes_the_stub_with_git() {
    let mut fake = with_runner();
    set_up(&mut fake);
    remove(&mut fake).unwrap();
    assert!(fake.node(&stub()).is_none());
    assert!(fake.node(&git()).is_none());
}

#[test]
fn a_stub_that_cant_be_placed_never_fails_the_set_up() {
    for runner_missing in [false, true] {
        let mut fake = with_runner();
        if runner_missing {
            fake.nodes.remove(&key(Path::new(RUNNER)));
        } else {
            fake.fail.push("place_file".to_owned());
        }
        let out = set_up(&mut fake);
        assert!(matches!(out[0].1, Recorded::Tree(_)), "{runner_missing}");
        assert!(fake.record.is_some(), "the record is written");
        assert!(fake.node(&stub()).is_none());
        assert!(
            fake.logs
                .iter()
                .any(|(error, line)| !error && line.contains("Git Bash stub")),
            "{:?}",
            fake.logs
        );
    }
}
