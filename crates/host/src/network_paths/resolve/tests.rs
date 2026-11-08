//! The walk on a fake disk that records each look: links read and walked, `..`, a missing name,
//! every way to a network location stopped before the walk touches it, and every failure
//! refused (fail-closed). Then once on the real disk.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{Disk, Entry, Fs, MAX_HOPS, Walked, walk};
use crate::network_paths::NetEnv;
use crate::network_paths::tests::plain_net;

/// One name on the fake disk.
#[derive(Debug, Clone)]
pub(crate) enum Fake {
    Dir,
    File,
    Link(PathBuf),
    /// A link whose target can't be read.
    BadLink,
    /// A name the walk may not look at (permission denied).
    Denied,
    /// A name the system rejects (`InvalidFilename`: Windows' answer for `*`, `?` or `:`).
    BadName,
}

/// A disk in memory that records every path it is asked about, in order.
#[derive(Debug, Default)]
pub(crate) struct FakeFs {
    pub(crate) names: BTreeMap<PathBuf, Fake>,
    pub(crate) touched: RefCell<Vec<PathBuf>>,
    /// The identities [`Fs::identity`] answers, by path.
    pub(crate) ids: BTreeMap<PathBuf, (u64, u128)>,
}

impl FakeFs {
    /// A disk holding `names`, each folder above them a folder too.
    pub(crate) fn with(names: &[(&str, Fake)]) -> Self {
        let mut fake = Self::default();
        for (name, kind) in names {
            let path = PathBuf::from(name);
            for above in path.ancestors().skip(1) {
                fake.names.entry(above.to_path_buf()).or_insert(Fake::Dir);
            }
            fake.names.insert(path, kind.clone());
        }
        fake
    }

    pub(crate) fn touched(&self) -> Vec<PathBuf> {
        self.touched.borrow().clone()
    }
}

impl Fs for FakeFs {
    fn entry(&self, path: &Path) -> io::Result<Entry> {
        self.touched.borrow_mut().push(path.to_path_buf());
        match self.names.get(path) {
            Some(Fake::Dir) => Ok(Entry::Dir),
            Some(Fake::File) => Ok(Entry::File),
            Some(Fake::Link(_) | Fake::BadLink) => Ok(Entry::Link),
            Some(Fake::Denied) => Err(io::ErrorKind::PermissionDenied.into()),
            Some(Fake::BadName) => Err(io::ErrorKind::InvalidFilename.into()),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn read_link(&self, path: &Path) -> io::Result<PathBuf> {
        self.touched.borrow_mut().push(path.to_path_buf());
        match self.names.get(path) {
            Some(Fake::Link(target)) => Ok(target.clone()),
            _ => Err(io::ErrorKind::InvalidInput.into()),
        }
    }

    fn identity(&self, path: &Path) -> Option<(u64, u128)> {
        self.touched.borrow_mut().push(path.to_path_buf());
        self.ids.get(path).copied()
    }
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

fn on(path: &str, fs: &FakeFs, net: &NetEnv) -> Walked {
    let real = |_: &Path| None;
    let disk = Disk {
        fs,
        real: &real,
        net,
    };
    walk(Path::new(path), &disk)
}

fn local(path: &str) -> Walked {
    Walked::Local(path.into())
}

#[test]
fn links_and_dot_dots_resolve_and_a_missing_name_is_kept() {
    let fs = FakeFs::with(&[
        ("/home/a/Deals/spa.docx", Fake::File),
        ("/home/a/link", Fake::Link("Deals".into())),
        ("/home/a/abs", Fake::Link("/home/a/Deals".into())),
    ]);
    let net = plain_net(false);
    let at = |p: &str| on(p, &fs, &net);
    assert_eq!(at("/home/a/Deals"), local("/home/a/Deals"));
    assert_eq!(at("/home/a/link/spa.docx"), local("/home/a/Deals/spa.docx"));
    assert_eq!(
        at("/home/a/abs/../Deals/spa.docx"),
        local("/home/a/Deals/spa.docx")
    );
    assert_eq!(
        at("/home/a/Deals/new/../draft.docx"),
        local("/home/a/Deals/draft.docx")
    );
    assert_eq!(
        at("/home/a/new/dir/x.docx"),
        local("/home/a/new/dir/x.docx")
    );
    assert_eq!(at("relative/x"), Walked::Refused);
}

/// A `..` back out of a missing name reaches names that are still looked at: on Windows
/// `C:\m\missing\..\link` opens `link`, so a link there to a share is refused.
#[test]
fn a_link_after_a_missing_name_and_a_dot_dot_is_still_checked() {
    let fs = FakeFs::with(&[("/m/link", Fake::Link(r"\\server\share".into()))]);
    let got = on("/m/missing/../link/x", &fs, &plain_net(false));
    assert_eq!(got, Walked::Refused);
    let want = paths(&["/m", "/m/missing", "/m/link", "/m/link"]);
    assert_eq!(fs.touched(), want);
}

#[test]
fn a_loop_an_unreadable_link_or_an_error_is_refused() {
    let net = plain_net(false);
    let looped = FakeFs::with(&[
        ("/a", Fake::Link("/b".into())),
        ("/b", Fake::Link("/a".into())),
    ]);
    assert_eq!(on("/a/x", &looped, &net), Walked::Refused);
    assert!(looped.touched().len() <= 2 * (MAX_HOPS + 1));
    let bad = FakeFs::with(&[("/m/bad", Fake::BadLink)]);
    assert_eq!(on("/m/bad/x", &bad, &net), Walked::Refused);
    assert_eq!(bad.touched(), paths(&["/m", "/m/bad", "/m/bad"]));
    let denied = FakeFs::with(&[("/m/locked", Fake::Denied)]);
    assert_eq!(on("/m/locked/x", &denied, &net), Walked::Refused);
    assert_eq!(denied.touched(), paths(&["/m", "/m/locked"]));
}

#[test]
fn a_network_path_is_never_touched() {
    let fs = FakeFs::default();
    let net = plain_net(false);
    for text in [
        "/net/host/share/x",
        "/Network/Servers/x",
        r"\\server\share\x",
        "smb://h/s",
    ] {
        assert_eq!(on(text, &fs, &net), Walked::Refused, "{text}");
    }
    assert_eq!(
        on("//server/share/x", &fs, &plain_net(true)),
        Walked::Refused
    );
    assert!(fs.touched().is_empty(), "{:?}", fs.touched());
}

#[test]
fn a_local_link_to_a_network_path_stops_at_the_link() {
    let net = NetEnv {
        remote_mounts: vec!["/Volumes/share".into()],
        ..plain_net(false)
    };
    for target in [
        "/net/host/share",
        "/Volumes/share/Deals",
        r"\\server\share",
        r"\\?\UNC\server\share",
        "../../../net/host",
    ] {
        let fs = FakeFs::with(&[("/home/a/x", Fake::Link(target.into()))]);
        assert_eq!(
            on("/home/a/x/doc.docx", &fs, &net),
            Walked::Refused,
            "{target}"
        );
        // The walk looked at the folders up to the link and read the link; nothing past it.
        let want = paths(&["/home", "/home/a", "/home/a/x", "/home/a/x"]);
        assert_eq!(fs.touched(), want, "{target}");
    }
}

#[test]
fn a_link_to_a_link_to_a_share_stops_at_the_second_link() {
    let fs = FakeFs::with(&[
        ("/home/a/x", Fake::Link("/srv/y".into())),
        ("/srv/y", Fake::Link("/net/host".into())),
    ]);
    assert_eq!(
        on("/home/a/x/doc.docx", &fs, &plain_net(false)),
        Walked::Refused
    );
    let want = paths(&[
        "/home",
        "/home/a",
        "/home/a/x",
        "/home/a/x",
        "/srv",
        "/srv/y",
        "/srv/y",
    ]);
    assert_eq!(fs.touched(), want);
}

#[test]
fn the_real_disk_resolves_links_and_dot_dots() {
    let dir = crate::pipeline::tests::support::TempDir::new("network-walk");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let deals = root.join("Deals");
    crate::write::create_dir_all(&deals).unwrap();
    crate::write::write_file(&deals.join("spa.docx"), b"x").unwrap();
    let net = plain_net(cfg!(windows));
    let disk = Disk::real(&net);
    let at = |p: &Path| walk(p, &disk);
    let spa = Walked::Local(deals.join("spa.docx"));
    assert_eq!(at(&deals), Walked::Local(deals.clone()));
    assert_eq!(
        at(&root.join("Deals").join("..").join("Deals").join("spa.docx")),
        spa
    );
    let missing = deals.join("new").join("..").join("draft.docx");
    assert_eq!(at(&missing), Walked::Local(deals.join("draft.docx")));
    #[cfg(unix)]
    {
        crate::write::symlink(&deals, &root.join("link")).unwrap();
        assert_eq!(at(&root.join("link").join("spa.docx")), spa);
    }
}

/// A name the system rejects (a shell word's wildcard, a URL's `https:`) is kept as written and
/// the walk goes on; a network name after it is still refused before it is touched.
#[test]
fn a_rejected_name_is_kept_and_later_names_still_checked() {
    let net = NetEnv {
        remote_mounts: vec!["/m/https:/share".into()],
        ..plain_net(false)
    };
    let fs = FakeFs::with(&[
        ("/m/docs/*.docx", Fake::BadName),
        ("/m/https:", Fake::BadName),
    ]);
    assert_eq!(on("/m/docs/*.docx", &fs, &net), local("/m/docs/*.docx"));
    assert_eq!(on("/m/https:/x/a", &fs, &net), local("/m/https:/x/a"));
    assert_eq!(on("/m/https:/share/a", &fs, &net), Walked::Refused);
    assert!(
        !fs.touched()
            .iter()
            .any(|p| p.starts_with("/m/https:/share"))
    );
}
