//! The data folder's identity forced unreadable through the fake disk (decision record 0030,
//! "Network locations"): the fence compares by names only, says so in its health, and still
//! refuses the data folder by every spelling the walk resolves (a junction or link to it, its
//! 8.3 short name as a path and as a shell word), while an ordinary matter passes. Only the data
//! folder's own identity, or a parent's, brings that fallback: an unreadable identity a request
//! leads to refuses the request. While failing, each check reads one identity, and once it reads
//! again the fallback clears. On the fake disk, so it runs everywhere (`C:\` on Windows).

use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::resolve::Disk;
use super::{DataFenceHealth, FenceCheck, candidates, candidates_words};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::tests::{Fake, FakeFs};
use crate::network_paths::resolve::{Entry, Fs};
use crate::network_paths::tests::plain_net;

/// `place` (`/`-separated) under this system's root: `/` on Unix, `C:\` on Windows.
fn at(place: &str) -> PathBuf {
    let root = PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" });
    place.split('/').fold(root, |path, name| path.join(name))
}

fn text(place: &str) -> String {
    at(place).display().to_string()
}

/// The data folder `d`, the matter `m`, an unrelated folder `other`, a link `m/j` to the data
/// folder and its 8.3 short name `u/CLAUSE~1`, with the identities of the data folder and the
/// short name (the same) and of `other`.
fn disk() -> FakeFs {
    let names: Vec<(String, Fake)> = [
        "d/state.sqlite",
        "m/brief.docx",
        "other/x",
        "u/CLAUSE~1/state.sqlite",
    ]
    .iter()
    .map(|place| (text(place), Fake::File))
    .collect();
    let names: Vec<(&str, Fake)> = names
        .iter()
        .map(|(name, kind)| (name.as_str(), kind.clone()))
        .collect();
    let mut fs = FakeFs::with(&names);
    fs.names.insert(at("m/j"), Fake::Link(at("d")));
    fs.ids.insert(at("d"), (1, 7));
    fs.ids.insert(at("u/CLAUSE~1"), (1, 7));
    fs.ids.insert(at("other"), (1, 8));
    fs.ids.insert(at("m"), (1, 9));
    fs
}

/// A request from the matter `m` naming `paths` and running `command`, sharing `health`.
fn request(health: &DataFenceHealth, paths: &[PathBuf], command: Option<&str>) -> FenceCheck {
    let raw = command.map(|command| json!({ "command": command }));
    FenceCheck {
        data: at("d"),
        allowed: Vec::new(),
        cwd: at("m"),
        home: None,
        paths: candidates(paths, raw.as_ref(), cfg!(windows)),
        roots: Vec::new(),
        words: candidates_words(raw.as_ref(), cfg!(windows)),
        health: health.clone(),
        vars: Vec::new(),
    }
}

/// The fake disk, counting the identities asked of it.
struct Counting<'a> {
    fs: &'a FakeFs,
    identities: RefCell<Vec<PathBuf>>,
}

impl Fs for Counting<'_> {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        self.fs.entry(path)
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.fs.read_link(path)
    }

    fn identity(&self, path: &Path) -> io::Result<(u64, u128)> {
        self.identities.borrow_mut().push(path.to_owned());
        self.fs.identity(path)
    }
}

/// Whether the fence refuses `check` on `fs`, the canonical step giving the short name's long
/// form when `long_names`; and the identities it read.
fn refused_on(check: &FenceCheck, fs: &FakeFs, long_names: bool) -> (bool, Vec<PathBuf>) {
    let net: NetEnv = plain_net(cfg!(windows));
    let short = at("u/CLAUSE~1");
    let real = |path: &Path| (long_names && path == short).then(|| at("d"));
    let counting = Counting {
        fs,
        identities: RefCell::new(Vec::new()),
    };
    let disk = Disk {
        fs: &counting,
        real: &real,
        net: &net,
    };
    let refused = check.touches_data_on(false, &disk);
    (refused, counting.identities.into_inner())
}

fn refused(check: &FenceCheck, fs: &FakeFs) -> bool {
    refused_on(check, fs, true).0
}

/// With the data folder's identity unreadable: the link alias (the walk follows it) and the 8.3
/// alias (the canonical step gives its long form), as a path and as a shell word, are refused
/// by name; an ordinary matter passes; the health says names only.
#[test]
fn with_the_data_folders_identity_unreadable_its_aliases_are_still_refused_by_name() {
    let mut fs = disk();
    fs.unreadable_ids.push(at("d"));
    let health = DataFenceHealth::default();
    let verb = if cfg!(windows) { "type" } else { "cat" };
    for (path, command) in [
        (Some("m/j/state.sqlite"), None),
        (Some("u/CLAUSE~1/state.sqlite"), None),
        (
            None,
            Some(format!("{verb} {}", text("u/CLAUSE~1/state.sqlite"))),
        ),
        (None, Some(format!("{verb} {}", text("m/j/state.sqlite")))),
        (Some("d/state.sqlite"), None),
    ] {
        let paths: Vec<PathBuf> = path.map(at).into_iter().collect();
        let check = request(&health, &paths, command.as_deref());
        assert!(refused(&check, &fs), "{path:?} {command:?}");
        assert!(health.names_only());
    }
    let ordinary = [at("m/brief.docx"), at("m/new.docx"), at("other/x")];
    let check = request(
        &health,
        &ordinary,
        Some(&format!("{verb} {}", text("m/brief.docx"))),
    );
    assert!(!refused(&check, &fs));
    let mut search = request(&health, &[], None);
    search.roots = vec![at("m"), at("other")];
    assert!(!refused(&search, &fs));
    assert!(health.names_only());
}

/// Only the data folder's own identity, or a parent's, brings the fallback. An identity a
/// request leads to that can't be read (an unrelated folder, the short name of the data folder
/// with no canonical step to give its long form, a search root) refuses that request, and the
/// fence stays on identities.
#[test]
fn an_unreadable_identity_a_request_leads_to_refuses_rather_than_falling_back() {
    let mut fs = disk();
    fs.unreadable_ids.push(at("other"));
    fs.unreadable_ids.push(at("u/CLAUSE~1"));
    let health = DataFenceHealth::default();
    let check = request(&health, &[at("other/x")], None);
    assert!(refused(&check, &fs));
    let alias = request(&health, &[at("u/CLAUSE~1/state.sqlite")], None);
    assert!(refused_on(&alias, &fs, false).0);
    let mut search = request(&health, &[], None);
    search.roots = vec![at("other")];
    assert!(refused(&search, &fs));
    assert!(!health.names_only());
    assert!(!refused(
        &request(&health, &[at("m/brief.docx")], None),
        &fs
    ));
    // The same alias, readable, is the data folder by identity.
    let readable = disk();
    assert!(refused_on(&alias, &readable, false).0);
}

/// The fallback starts when the data folder's identity can't be read, costs one identity read
/// per check while it lasts (the one that failed, retried), and clears at the first check after
/// it reads again; once all read, nothing is read again for them.
#[test]
fn the_flag_is_set_retried_once_per_check_and_clears_once_the_identity_reads() {
    let mut failing = disk();
    failing.unreadable_ids.push(at("d"));
    let health = DataFenceHealth::default();
    let check = request(&health, &[at("m/brief.docx")], None);
    let (denied, read) = refused_on(&check, &failing, true);
    assert!(!denied);
    assert!(health.names_only());
    assert!(read.contains(&at("d")), "{read:?}");
    for _ in 0..2 {
        let (denied, read) = refused_on(&check, &failing, true);
        assert!(!denied);
        assert_eq!(read, vec![at("d")]);
        assert!(health.names_only());
    }
    let readable = disk();
    let (denied, _) = refused_on(&check, &readable, true);
    assert!(!denied);
    assert!(!health.names_only());
    let (denied, read) = refused_on(&check, &readable, true);
    assert!(!denied);
    assert!(
        !read.iter().any(|path| at("d").starts_with(path)),
        "{read:?}"
    );
    // A parent's identity unreadable is the same fallback.
    let mut parent = disk();
    parent
        .unreadable_ids
        .push(at("d").parent().unwrap().to_owned());
    let health = DataFenceHealth::default();
    let check = request(&health, &[at("m/brief.docx")], None);
    assert!(!refused_on(&check, &parent, true).0);
    assert!(health.names_only());
    assert!(refused(
        &request(&health, &[at("m/j/state.sqlite")], None),
        &parent
    ));
}
