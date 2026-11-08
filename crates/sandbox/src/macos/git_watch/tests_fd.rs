//! The real file system by descriptor, on this Mac: a link is judged without following any,
//! so `a -> b/../x` with `b -> .` leads out; a folder swapped for a link to `/net/x` or to a
//! folder outside the matter after its parent's listing is neither followed nor listed; nothing
//! at or below `/net` or `/Network`, nor the automounted `/System/Volumes/Data/home`, is
//! entered; renames never replace and never go through a folder swapped for a link; and the
//! state file is never read through a link.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests make their temp folders, links and swaps with std"
)]

use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use super::check::walk;
use super::{Fs as _, Kind, os};

/// A temp folder holding `matter` and `outside`, removed on drop.
struct Scratch {
    root: PathBuf,
    matter: PathBuf,
    outside: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root = PathBuf::from(format!("/private/tmp/cgw-fd-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let matter = root.join("matter");
        let outside = root.join("outside");
        std::fs::create_dir_all(&matter).unwrap();
        std::fs::create_dir_all(outside.join("deep")).unwrap();
        std::fs::create_dir_all(outside.join("deep/.git")).unwrap();
        Scratch {
            root,
            matter,
            outside,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.root));
    }
}

/// The folders a walk of `top` visits; `swap` runs at each visit.
fn visited(top: &Path, mut swap: impl FnMut(&Path)) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    walk(&os::Real, top, &|_, _| false, &mut |dir, _, _| {
        seen.push(dir.to_owned());
        swap(dir);
        true
    });
    seen
}

#[test]
fn a_link_is_judged_without_following_any() {
    let s = Scratch::new("judge");
    let m = &s.matter;
    std::fs::write(m.join("x"), "x").unwrap();
    std::fs::write(s.root.join("x"), "the matter's parent").unwrap();
    std::fs::create_dir(m.join("docs")).unwrap();
    symlink(".", m.join("b")).unwrap();
    symlink("b/../x", m.join("a")).unwrap();
    symlink("x", m.join("plain")).unwrap();
    symlink("plain", m.join("chain")).unwrap();
    symlink("docs/../x", m.join("up_and_back")).unwrap();
    symlink(m.join("docs"), m.join("absolute_inside")).unwrap();
    symlink(&s.outside, m.join("absolute_out")).unwrap();
    symlink("../outside", m.join("above")).unwrap();
    symlink("nowhere", m.join("dangling")).unwrap();
    symlink("b/x", m.join("through_a_link")).unwrap();
    symlink("/net/x", m.join("net")).unwrap();
    let out = |name: &str| os::Real.leads_out(&m.join(name), m);
    assert!(out("a"), "b -> . then .. goes to the matter's parent");
    assert!(!out("plain") && !out("chain") && !out("up_and_back"));
    assert!(!out("absolute_inside"));
    assert!(out("absolute_out") && out("above") && out("dangling") && out("net"));
    assert!(
        out("through_a_link"),
        "a link on the way counts as leading out"
    );
    assert!(out("x"), "not a link: nothing to judge");
    // A link whose folder is swapped for a link is judged out too.
    std::fs::create_dir(m.join("sub")).unwrap();
    symlink("../x", m.join("sub/up")).unwrap();
    assert!(!out("sub/up"));
    std::fs::rename(m.join("sub"), s.root.join("sub")).unwrap();
    symlink(s.root.join("sub"), m.join("sub")).unwrap();
    assert!(out("sub/up"));
}

#[test]
fn a_queued_folder_swapped_for_a_link_is_neither_followed_nor_listed() {
    for (n, target) in ["/net/x", "outside"].into_iter().enumerate() {
        let s = Scratch::new(&format!("swap-{n}"));
        let target = if target == "outside" {
            s.outside.clone()
        } else {
            PathBuf::from(target)
        };
        let p = s.matter.join("p");
        std::fs::create_dir_all(p.join("a/below")).unwrap();
        let seen = visited(&s.matter, |dir| {
            if dir == p {
                std::fs::rename(p.join("a"), s.root.join("a")).unwrap();
                symlink(&target, p.join("a")).unwrap();
            }
        });
        assert_eq!(seen, [s.matter.clone(), p.clone()], "{target:?}");
    }
}

#[test]
fn a_folder_swapped_once_listed_is_gone_into_from_what_was_listed() {
    let s = Scratch::new("held");
    let p = s.matter.join("p");
    std::fs::create_dir_all(p.join("a")).unwrap();
    // Once `p` is listed it moves out and a link to `outside` takes its place: `p/a` is
    // opened from the `p` that was listed, never through the link, so `outside/deep` is
    // never reached.
    let seen = visited(&s.matter, |dir| {
        if dir == p {
            std::fs::rename(&p, s.root.join("p")).unwrap();
            symlink(&s.outside, &p).unwrap();
        }
    });
    assert_eq!(seen, [s.matter.clone(), p.clone(), p.join("a")]);
}

#[test]
fn nothing_at_or_below_net_or_network_is_touched() {
    for top in ["/net", "/net/x", "/Network/Servers/x", "/NET/x"] {
        let top = Path::new(top);
        assert!(os::off_limits(top), "{top:?}");
        assert_eq!(visited(top, |_| {}), Vec::<PathBuf>::new());
        let refused = os::Real.list(top).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::Unsupported, "{refused}");
        assert!(os::Real.meta(&top.join("y")).is_none());
        assert!(!os::Real.local(top));
        let opened = os::Real.open_dir(top).map(|found| found.is_some());
        assert_eq!(opened.unwrap_err().kind(), io::ErrorKind::Unsupported);
    }
    assert!(!os::off_limits(Path::new("/private/tmp/net")));
}

#[test]
fn the_home_automount_is_never_entered() {
    let home = Path::new("/System/Volumes/Data/home");
    let Some(meta) = std::fs::symlink_metadata(home)
        .ok()
        .filter(std::fs::Metadata::is_dir)
    else {
        println!("no {} on this Mac", home.display());
        return;
    };
    assert!(meta.is_dir());
    let refused = os::Real.open_dir(home).map(|found| found.is_some());
    assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert!(!os::Real.local(home));
    assert_eq!(visited(home, |_| {}), Vec::<PathBuf>::new());
    // From the Data volume, the walk reaches `home` and refuses it before opening it.
    let data = Path::new("/System/Volumes/Data");
    let (folder, _) = os::Real.open_dir(data).unwrap().unwrap();
    let name = home.file_name().unwrap();
    let refused = os::Real
        .open_child(&folder, name, home)
        .map(|found| found.is_some());
    assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::Unsupported);
}

#[test]
fn renames_never_replace_nor_go_through_a_swapped_folder() {
    let s = Scratch::new("rename");
    let m = &s.matter;
    std::fs::create_dir_all(m.join("src")).unwrap();
    std::fs::create_dir_all(m.join("dst")).unwrap();
    std::fs::write(m.join("src/item"), "item").unwrap();
    std::fs::write(m.join("dst/taken"), "taken").unwrap();
    let error = os::Real
        .rename_new(&m.join("src/item"), &m.join("dst/taken"))
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(m.join("dst/taken")).unwrap(), b"taken");
    // The destination's folder swapped for a link to a folder outside.
    std::fs::rename(m.join("dst"), s.root.join("dst")).unwrap();
    symlink(&s.outside, m.join("dst")).unwrap();
    assert!(
        os::Real
            .rename_new(&m.join("src/item"), &m.join("dst/new"))
            .is_err()
    );
    assert!(!s.outside.join("new").exists() && m.join("src/item").exists());
    // The source's folder swapped for a link: nothing outside moves.
    std::fs::write(s.outside.join("victim"), "v").unwrap();
    std::fs::rename(m.join("src"), s.root.join("src")).unwrap();
    symlink(&s.outside, m.join("src")).unwrap();
    std::fs::create_dir(m.join("to")).unwrap();
    assert!(
        os::Real
            .rename_new(&m.join("src/victim"), &m.join("to/victim"))
            .is_err()
    );
    assert!(s.outside.join("victim").exists() && !m.join("to/victim").exists());
    // A plain move goes.
    std::fs::write(m.join("to/one"), "1").unwrap();
    os::Real
        .rename_new(&m.join("to/one"), &m.join("to/two"))
        .unwrap();
    assert!(m.join("to/two").exists());
}

#[test]
fn reads_and_writes_never_go_through_a_link() {
    let s = Scratch::new("io");
    let m = &s.matter;
    std::fs::write(s.outside.join("file"), "outside").unwrap();
    symlink(&s.outside, m.join("door")).unwrap();
    assert_eq!(os::Real.meta(&m.join("door")).unwrap().kind, Kind::Link);
    assert!(os::Real.meta(&m.join("door/file")).is_none());
    assert!(os::Real.list(&m.join("door")).is_err());
    assert!(os::Real.write_new(&m.join("door/new"), b"x").is_err());
    assert!(os::Real.make_dir(&m.join("door/new")).is_err());
    assert!(os::Real.touch(&m.join("door/canary")).is_err());
    assert!(os::Real.make_dirs(&m.join("door/a/b")).is_err());
    assert!(os::Real.remove_link(&m.join("door/file")).is_err());
    assert!(!s.outside.join("new").exists() && !s.outside.join("a").exists());
    assert_eq!(os::Real.read_link(&m.join("door")).unwrap(), s.outside);
    std::fs::write(m.join("plain"), "p").unwrap();
    assert!(
        os::Real.remove_link(&m.join("plain")).is_err(),
        "not a link"
    );
    os::Real.remove_link(&m.join("door")).unwrap();
    assert!(s.outside.join("file").exists());
    // The state file: read and replaced by its folder's descriptor, never through a link.
    let state = m.join("matters/state.json");
    let lock = os::lock(&m.join("matters/state.lock")).unwrap();
    os::replace(&state, b"{}").unwrap();
    assert_eq!(os::read_small(&state).unwrap(), b"{}");
    drop(lock);
    symlink(s.outside.join("file"), m.join("matters/linked.json")).unwrap();
    assert!(os::read_small(&m.join("matters/linked.json")).is_err());
}
