//! The `SYSTEM` side's heal of a squatted drive folder (`heal.rs`), over a fake that keeps each
//! folder's contents as names no call can reach and fails a test that opens anything but a
//! drive folder itself: a squatted folder is locked down and renamed by its handle, unopened
//! inside, and the lawyer's made afresh; a link or a file is refused; the lawyer's own folder, a
//! missing one and another lawyer's are left as they are.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::drive_folder_name;
use super::super::heal::{Facts, Healed, Healer, heal, heal_all};
use crate::windows::sids::Sid;

/// The lawyer.
const LAWYER: &str = "S-1-5-21-1-2-3-1001";
/// Another lawyer of the machine.
const LAWYER_B: &str = "S-1-5-21-1-2-3-1002";
/// Whoever made the folder first.
const SQUATTER: &str = "S-1-5-21-9-9-9-1500";
/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// The DACL the fake gives a locked-down folder.
const LOCKED: &str = "SYSTEM and Administrators";
/// What the fake keeps inside a squatted folder.
const INSIDE: [&str; 2] = ["secret-brief.docx", "sub"];

/// One folder or file the fake knows, at a drive's root.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    reparse: bool,
    folder: bool,
    owner: String,
    dacl: String,
    /// Its contents' names: no [`Healer`] call reaches them.
    inside: Vec<String>,
}

impl Node {
    fn folder(owner: &str) -> Node {
        Node {
            reparse: false,
            folder: true,
            owner: owner.to_owned(),
            dacl: "the squatter's".to_owned(),
            inside: INSIDE.map(str::to_owned).to_vec(),
        }
    }
}

/// The fake: nodes by id, the path each is at, every path opened, every call and log line.
#[derive(Default)]
struct Fake {
    roots: Vec<PathBuf>,
    nodes: RefCell<BTreeMap<u64, Node>>,
    at: RefCell<BTreeMap<PathBuf, u64>>,
    opened: RefCell<Vec<PathBuf>>,
    calls: RefCell<Vec<String>>,
    logs: RefCell<Vec<(bool, String)>>,
    /// Whether [`Healer::make_fresh`] fails.
    fresh_fails: bool,
}

impl Fake {
    fn new(roots: &[&Path]) -> Fake {
        Fake {
            roots: roots.iter().map(|root| root.to_path_buf()).collect(),
            ..Fake::default()
        }
    }

    fn put(&self, path: PathBuf, node: Node) {
        let id = u64::try_from(self.nodes.borrow().len()).unwrap();
        self.nodes.borrow_mut().insert(id, node);
        self.at.borrow_mut().insert(path, id);
    }

    fn node(&self, path: &Path) -> Option<Node> {
        let id = *self.at.borrow().get(path)?;
        self.nodes.borrow().get(&id).cloned()
    }
}

impl Healer for Fake {
    type Opened = u64;

    fn open(&self, path: &Path) -> io::Result<Option<u64>> {
        let parent = path.parent().unwrap();
        assert!(
            self.roots.iter().any(|root| root == parent),
            "opened something that isn't at a drive's root: {}",
            path.display()
        );
        self.opened.borrow_mut().push(path.to_owned());
        Ok(self.at.borrow().get(path).copied())
    }

    fn facts(&self, opened: &u64) -> io::Result<Facts> {
        self.calls.borrow_mut().push("facts".to_owned());
        let node = self.nodes.borrow().get(opened).cloned().unwrap();
        Ok(Facts {
            reparse: node.reparse,
            folder: node.folder,
            owner: node.owner,
        })
    }

    fn lock_down(&self, opened: &u64) -> io::Result<()> {
        self.calls.borrow_mut().push("lock_down".to_owned());
        let mut nodes = self.nodes.borrow_mut();
        let node = nodes.get_mut(opened).unwrap();
        SYSTEM.clone_into(&mut node.owner);
        LOCKED.clone_into(&mut node.dacl);
        Ok(())
    }

    fn rename(&self, opened: &u64, name: &OsStr) -> io::Result<()> {
        self.calls.borrow_mut().push("rename".to_owned());
        let mut at = self.at.borrow_mut();
        let from = at
            .iter()
            .find(|(_, id)| *id == opened)
            .map(|(path, _)| path.clone())
            .unwrap();
        let to = from.parent().unwrap().join(name);
        if at.contains_key(&to) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        at.remove(&from);
        at.insert(to, *opened);
        Ok(())
    }

    fn make_fresh(&self, path: &Path, lawyer: &Sid) -> io::Result<()> {
        self.calls.borrow_mut().push("make_fresh".to_owned());
        if self.fresh_fails || self.at.borrow().contains_key(path) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        let mut node = Node::folder(lawyer.as_str());
        node.inside.clear();
        "the drive DACL".clone_into(&mut node.dacl);
        self.put(path.to_owned(), node);
        Ok(())
    }

    fn log(&self, error: bool, line: &str) {
        self.logs.borrow_mut().push((error, line.to_owned()));
    }
}

/// 2026-11-09 10:15:00 UTC.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_mins(29_903_655)
}

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// `D:\` as the tests' drive root, and the lawyer's drive folder on it.
fn drive() -> (PathBuf, PathBuf) {
    let root = PathBuf::from("drive-d");
    let folder = root.join(drive_folder_name(LAWYER));
    (root, folder)
}

#[test]
fn a_squatted_drive_folder_is_set_aside_unopened_and_made_afresh() {
    let (root, folder) = drive();
    let fake = Fake::new(&[&root]);
    fake.put(folder.clone(), Node::folder(SQUATTER));

    let healed = heal(&fake, &root, &sid(LAWYER), now());

    let aside = format!("{}.squatted-20261109T101500Z", drive_folder_name(LAWYER));
    assert_eq!(healed, Healed::SetAside(aside.clone()));
    assert_eq!(
        fake.opened.borrow().as_slice(),
        std::slice::from_ref(&folder),
        "only the folder itself"
    );
    let set_aside = fake.node(&root.join(&aside)).unwrap();
    assert_eq!(set_aside.owner, SYSTEM);
    assert_eq!(set_aside.dacl, LOCKED);
    assert_eq!(set_aside.inside, INSIDE, "what it holds is as it was");
    let fresh = fake.node(&folder).unwrap();
    assert_eq!((fresh.owner.as_str(), fresh.inside.len()), (LAWYER, 0));
    assert_eq!(
        *fake.calls.borrow(),
        ["facts", "lock_down", "rename", "make_fresh"]
    );
    let logs = fake.logs.borrow();
    let [(false, line)] = logs.as_slice() else {
        panic!("one info line: {logs:?}");
    };
    assert!(line.contains(SQUATTER) && line.contains(&aside), "{line}");
    assert!(INSIDE.iter().all(|name| !line.contains(name)), "{line}");
}

#[test]
fn a_reparse_point_or_a_file_squat_is_refused_not_healed() {
    for (reparse, folder) in [(true, true), (true, false), (false, false)] {
        let (root, path) = drive();
        let fake = Fake::new(&[&root]);
        let node = Node {
            reparse,
            folder,
            ..Node::folder(SQUATTER)
        };
        fake.put(path.clone(), node.clone());

        let healed = heal(&fake, &root, &sid(LAWYER), now());

        assert_eq!(healed, Healed::Refused, "{reparse} {folder}");
        assert_eq!(*fake.calls.borrow(), ["facts"], "nothing changed");
        assert_eq!(fake.node(&path), Some(node), "left as it was");
        assert_eq!(fake.at.borrow().len(), 1, "nothing renamed or made");
        assert_eq!(fake.logs.borrow().len(), 1);
        assert!(fake.logs.borrow()[0].0, "logged as an error");
    }
}

#[test]
fn the_lawyers_own_folder_and_a_missing_one_are_left_alone() {
    let (root, folder) = drive();
    let fake = Fake::new(&[&root]);
    assert_eq!(heal(&fake, &root, &sid(LAWYER), now()), Healed::Missing);
    fake.put(folder.clone(), Node::folder(LAWYER));
    assert_eq!(heal(&fake, &root, &sid(LAWYER), now()), Healed::Lawyers);
    assert_eq!(*fake.calls.borrow(), ["facts"]);
    assert!(fake.logs.borrow().is_empty());
    assert_eq!(fake.node(&folder).unwrap().owner, LAWYER);
}

#[test]
fn lawyer_b_is_unaffected_by_lawyer_as_squatted_folder() {
    let (root, folder) = drive();
    let theirs = root.join(drive_folder_name(LAWYER_B));
    let fake = Fake::new(&[&root]);
    fake.put(folder.clone(), Node::folder(SQUATTER));
    fake.put(theirs.clone(), Node::folder(LAWYER_B));
    let before = fake.node(&theirs);

    let healed = heal_all(
        &fake,
        std::slice::from_ref(&root),
        &[sid(LAWYER), sid(LAWYER_B)],
        now(),
    );

    assert!(matches!(
        healed.as_slice(),
        [Healed::SetAside(_), Healed::Lawyers]
    ));
    assert_eq!(fake.node(&theirs), before, "lawyer B's folder is as it was");
    assert_eq!(fake.node(&folder).unwrap().owner, LAWYER);

    // B's folder is never even opened when only A is healed.
    let fake = Fake::new(&[&root]);
    fake.put(theirs.clone(), Node::folder(SQUATTER));
    heal_all(&fake, &[root], &[sid(LAWYER)], now());
    assert_eq!(*fake.opened.borrow(), [folder]);
    assert_eq!(fake.node(&theirs).unwrap().owner, SQUATTER);
}

#[test]
fn a_fresh_folder_that_cant_be_made_leaves_the_squat_set_aside_and_one_line() {
    let (root, folder) = drive();
    let fake = Fake {
        fresh_fails: true,
        ..Fake::new(&[&root])
    };
    fake.put(folder.clone(), Node::folder(SQUATTER));

    let healed = heal(&fake, &root, &sid(LAWYER), now());

    assert!(matches!(healed, Healed::SetAsideOnly(_)), "{healed:?}");
    assert!(fake.node(&folder).is_none());
    let logs = fake.logs.borrow();
    assert_eq!(logs.len(), 1);
    assert!(logs[0].0, "logged as an error");
}
