//! The walk on a fake disk that records each look: links read and walked, `..`, a missing tail,
//! a loop, and every way to a network location stopped before the walk touches it. Then once
//! on the real disk.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::{Entry, Fs, RealFs, Resolved, resolve};
use crate::actor::turn::denials::network::NetEnv;
use crate::actor::turn::denials::network::tests::plain_net;

/// One name on the fake disk.
#[derive(Debug, Clone)]
pub(crate) enum Fake {
    Dir,
    File,
    Link(PathBuf),
    /// A link whose target can't be read.
    BrokenLink,
    /// A name the disk answers with an error other than a missing one.
    Failing,
}

/// A disk in memory that records every path it is asked about, in order.
#[derive(Debug, Default)]
pub(crate) struct FakeFs {
    pub(crate) names: BTreeMap<PathBuf, Fake>,
    pub(crate) touched: RefCell<Vec<PathBuf>>,
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
            Some(Fake::Link(_) | Fake::BrokenLink) => Ok(Entry::Link),
            Some(Fake::Failing) => Err(io::ErrorKind::PermissionDenied.into()),
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
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

fn local(path: &str, is_file: bool) -> Resolved {
    Resolved::Local {
        path: path.into(),
        is_file,
    }
}

#[test]
fn links_and_dot_dots_resolve_and_a_missing_tail_is_a_file() {
    let fs = FakeFs::with(&[
        ("/home/a/Deals/spa.docx", Fake::File),
        ("/home/a/link", Fake::Link("Deals".into())),
        ("/home/a/abs", Fake::Link("/home/a/Deals".into())),
    ]);
    let net = plain_net(false);
    let at = |p: &str| resolve(Path::new(p), &fs, &net);
    assert_eq!(at("/home/a/Deals"), local("/home/a/Deals", false));
    assert_eq!(
        at("/home/a/link/spa.docx"),
        local("/home/a/Deals/spa.docx", true)
    );
    assert_eq!(
        at("/home/a/abs/../Deals/spa.docx"),
        local("/home/a/Deals/spa.docx", true)
    );
    assert_eq!(
        at("/home/a/Deals/new/../draft.docx"),
        local("/home/a/Deals/draft.docx", true)
    );
    assert_eq!(at("relative/x"), Resolved::Unresolved);
}

#[test]
fn a_link_loop_leads_nowhere() {
    let fs = FakeFs::with(&[
        ("/a", Fake::Link("/b".into())),
        ("/b", Fake::Link("/a".into())),
    ]);
    assert_eq!(
        resolve(Path::new("/a/x"), &fs, &plain_net(false)),
        Resolved::Unresolved
    );
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
        assert_eq!(
            resolve(Path::new(text), &fs, &net),
            Resolved::Network,
            "{text}"
        );
    }
    let windows = plain_net(true);
    assert_eq!(
        resolve(Path::new("//server/share/x"), &fs, &windows),
        Resolved::Network
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
        let got = resolve(Path::new("/home/a/x/doc.docx"), &fs, &net);
        assert_eq!(got, Resolved::Network, "{target}");
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
    let got = resolve(Path::new("/home/a/x/doc.docx"), &fs, &plain_net(false));
    assert_eq!(got, Resolved::Network);
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
fn with_windows_rules_dot_dots_go_first_by_text_so_a_link_past_a_missing_name_is_read() {
    // Win32 opens `/m/link/x` for `/m/missing/../link/x` and follows `link` to the share.
    let fs = FakeFs::with(&[("/m/link", Fake::Link(r"\\server\share".into()))]);
    let got = resolve(Path::new("/m/missing/../link/x"), &fs, &plain_net(true));
    assert_eq!(got, Resolved::Network);
    // The missing name is never looked at, and nothing past the link.
    assert_eq!(fs.touched(), paths(&["/m", "/m/link", "/m/link"]));
}

#[cfg(windows)]
#[test]
fn a_windows_drive_path_past_a_missing_name_reads_the_link() {
    let fs = FakeFs::with(&[(r"C:\m\link", Fake::Link(r"\\server\share".into()))]);
    let got = resolve(Path::new(r"C:\m\missing\..\link\x"), &fs, &plain_net(true));
    assert_eq!(got, Resolved::Network);
    assert_eq!(fs.touched(), paths(&[r"C:\m", r"C:\m\link", r"C:\m\link"]));
}

#[test]
fn every_name_past_a_missing_one_is_still_checked_by_its_text() {
    let fs = FakeFs::with(&[("/m", Fake::Dir)]);
    let net = plain_net(false);
    let got = resolve(Path::new("/m/missing/../../net/host/x"), &fs, &net);
    assert_eq!(got, Resolved::Network);
    let deeper = resolve(Path::new("/m/missing/a/../../../net"), &fs, &net);
    assert_eq!(deeper, Resolved::Network);
    // Only the names up to the missing one were looked at.
    assert_eq!(
        fs.touched(),
        paths(&["/m", "/m/missing", "/m", "/m/missing"])
    );
}

#[test]
fn a_loop_a_broken_link_or_an_error_is_unresolved() {
    let net = plain_net(false);
    let fs = FakeFs::with(&[
        ("/a", Fake::Link("/b".into())),
        ("/b", Fake::Link("/a".into())),
        ("/home/a/broken", Fake::BrokenLink),
        ("/home/a/locked", Fake::Failing),
    ]);
    for path in ["/a/x", "/home/a/broken/x", "/home/a/locked/x"] {
        assert_eq!(
            resolve(Path::new(path), &fs, &net),
            Resolved::Unresolved,
            "{path}"
        );
    }
}

#[test]
fn the_real_disk_resolves_links_and_dot_dots() {
    let dir = crate::pipeline::tests::support::TempDir::new("denial-resolve");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let deals = root.join("Deals");
    crate::write::create_dir_all(&deals).unwrap();
    crate::write::write_file(&deals.join("spa.docx"), b"x").unwrap();
    let net = plain_net(cfg!(windows));
    let at = |p: &Path| resolve(p, &RealFs, &net);
    let spa = Resolved::Local {
        path: deals.join("spa.docx"),
        is_file: true,
    };
    assert_eq!(
        at(&deals),
        Resolved::Local {
            path: deals.clone(),
            is_file: false
        }
    );
    assert_eq!(
        at(&root.join("Deals").join("..").join("Deals").join("spa.docx")),
        spa
    );
    let missing = deals.join("new").join("..").join("draft.docx");
    assert_eq!(
        at(&missing),
        Resolved::Local {
            path: deals.join("draft.docx"),
            is_file: true
        }
    );
    #[cfg(unix)]
    {
        crate::write::symlink(&deals, &root.join("link")).unwrap();
        assert_eq!(at(&root.join("link").join("spa.docx")), spa);
    }
}
