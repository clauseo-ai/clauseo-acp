//! Rule 8 over the stand-in volume: every read the live watch makes goes through handles from
//! the watched folder, never by a path that could follow a link partway, and a link's target or a
//! `commondir`'s folder is told inside the matter by identity, not by spelling. The stand-in
//! notes what it opened ([`Posix::opened`]) and what it read by path ([`Posix::by_path`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::links::link;
use super::live::{calm, origins, reported, watched};
use super::posix::Posix;
use super::{exit, launch, launch_as, none, world};
use crate::SetAsideWhat;
use crate::windows::set_aside::live::{Ending, Watched};
use crate::windows::set_aside::scan::remote;
use crate::windows::test_world::matter_one;

const HEAD: &str = "ref: refs/heads/main\n";

/// The stand-in, with `MATTE~1` a short name for `matter`, as NTFS's 8.3 names are.
fn with_short_name() -> Posix {
    let volume = Posix::new(true);
    volume
        .aliases
        .borrow_mut()
        .insert(OsString::from("MATTE~1"), OsString::from("matter"));
    volume
}

/// Whether the stand-in read nothing below `root` by path.
fn nothing_by_path_below(volume: &Posix, root: &Path) -> bool {
    !volume.by_path.borrow().iter().any(|p| p.starts_with(root))
}

/// A link spelled otherwise than the matter's own path (another case, `.` and `..`, a short
/// name, relative words) that leads inside is left; one beside the matter, above it, or at a
/// missing name leads out and goes.
#[test]
fn a_link_spelled_otherwise_that_leads_inside_is_judged_by_identity() {
    let (w, root, data) = world("live-read-identity");
    w.dir.file("matter/sub/notes.md", "");
    w.dir.dir("elsewhere");
    let watch = watched(&root, &data);
    let parent = root.parent().unwrap();

    link(&w, "matter/case", &parent.join("MATTER/sub"));
    link(&w, "matter/dots", &root.join("sub/../sub/."));
    link(&w, "matter/short", &parent.join("MATTE~1/sub"));
    link(&w, "matter/words", Path::new("../matter/sub"));
    link(&w, "matter/beside", &parent.join("elsewhere"));
    link(&w, "matter/above", &root.join("sub/../.."));
    link(&w, "matter/missing", &root.join("nothing"));
    let volume = with_short_name();
    let names = [
        "case", "dots", "short", "words", "beside", "above", "missing",
    ];
    let live = watch.changed(&volume, &reported(&names), exit(), &calm());

    let out = ["above", "beside", "missing"].map(|name| root.join(name));
    assert_eq!(origins(&live), out);
    assert!(live.moved.iter().all(|m| m.what == SetAsideWhat::LinkOut));
    let opened = volume.opened.borrow();
    assert!(opened.contains(&PathBuf::from("/")), "walked from the top");
    assert!(nothing_by_path_below(&volume, &root));
}

/// A link to a network path (`\\server\share`, `//server/share`, `\\?\UNC\...`) leads out by its
/// words alone: nothing of it is ever opened, so no share is ever asked to sign the lawyer in.
#[test]
fn a_link_to_a_network_path_leads_out_without_any_open_of_it() {
    let (w, root, data) = world("live-read-network");
    let watch = watched(&root, &data);

    link(&w, "matter/unc", Path::new(r"\\server\share\x"));
    link(&w, "matter/slashes", Path::new("//server/share"));
    link(&w, "matter/verbatim", Path::new(r"\\?\UNC\server\share"));
    let volume = Posix::new(true);
    let live = watch.changed(
        &volume,
        &reported(&["unc", "slashes", "verbatim"]),
        exit(),
        &calm(),
    );

    let out = ["slashes", "unc", "verbatim"].map(|name| root.join(name));
    assert_eq!(origins(&live), out);
    let opened = volume.opened.borrow();
    assert!(
        !opened
            .iter()
            .any(|p| p.to_string_lossy().contains("server")),
        "{opened:?}"
    );
}

/// The words that name a network path, a device or a volume by its name, read as Windows reads
/// them, and the ones that don't.
#[test]
fn network_and_device_paths_are_told_by_their_words() {
    let remote_words = [
        r"\\server\share",
        "//server/share",
        r"\/server",
        r"\\?\UNC\server\share",
        r"\??\UNC\server\share",
        r"\\.\pipe\x",
        r"\\?\Volume{0b1c}\x",
        r"\\?\GLOBALROOT\Device\x",
        r"\??\C",
    ];
    for words in remote_words {
        assert!(remote(words.as_ref()), "{words}");
    }
    let local = [
        r"\\?\C:\x",
        r"\??\c:\x",
        r"\\?\D:",
        r"C:\x",
        r"..\x",
        "/abs/x",
        "x",
    ];
    for words in local {
        assert!(!remote(words.as_ref()), "{words}");
    }
}

/// A reported path with a link partway (`a\hop` made a link to a folder outside) is never read
/// through the link: the reads stop at it, so what is past it is never opened, moved or read by
/// path, and the items reported there count as gone.
#[test]
fn a_path_with_a_link_partway_is_never_followed() {
    let (w, root, data) = world("live-read-partway");
    w.dir.dir("matter/a");
    w.dir.file("elsewhere/.git/HEAD", HEAD);
    w.dir.file("elsewhere/HEAD", HEAD);
    let elsewhere = w.canonical("elsewhere");
    w.dir.symlink("elsewhere/out", Path::new("/"));
    let watch = watched(&root, &data);

    link(&w, "matter/a/hop", &elsewhere);
    let volume = Posix::new(true);
    let hooks = calm();
    let paths = reported(&["a/hop/.git", "a/hop/out", "a/hop/HEAD"]);
    let live = watch.changed(&volume, &paths, exit(), &hooks);

    assert!(live.moved.is_empty() && live.failed.is_empty(), "{live:?}");
    assert_eq!(
        live.gone,
        ["a/hop/.git", "a/hop/HEAD"].map(PathBuf::from),
        "past the link: not in the matter"
    );
    assert_eq!(*hooks.ends.borrow(), [Ending::Found]);
    assert!(elsewhere.join(".git/HEAD").is_file() && elsewhere.join("HEAD").is_file());
    assert!(std::fs::symlink_metadata(elsewhere.join("out")).is_ok());
    let opened = volume.opened.borrow();
    let hop = root.join("a/hop");
    let past = |p: &PathBuf| p.starts_with(&elsewhere) || (p.starts_with(&hop) && *p != hop);
    assert!(!opened.iter().any(past), "{opened:?}");
    assert!(nothing_by_path_below(&volume, &root));
    assert!(nothing_by_path_below(&volume, &elsewhere));
}

/// A launch `commondir` that names a folder in the matter by another spelling (a short name;
/// another case with `.`) names that folder: what git reads made there goes, as it does when the
/// `commondir` spells the folder as the matter does.
#[test]
fn a_commondir_spelled_otherwise_names_the_same_folder() {
    let (w, root, data) = world("live-read-common");
    let parent = root.parent().unwrap();
    let short = parent.join("MATTE~1/common");
    w.dir.file("matter/wt/HEAD", HEAD);
    w.dir.file(
        "matter/wt/commondir",
        &format!("{}\n", short.to_str().unwrap()),
    );
    w.dir.file("matter/wt2/HEAD", HEAD);
    w.dir
        .file("matter/wt2/commondir", "../../MATTER/./shared\n");
    let volume = with_short_name();
    assert_eq!(
        launch_as(&volume, &root, &data, "s1", &[], launch()).unwrap(),
        none()
    );
    let watch = Watched::new(&volume, &root, &data, &matter_one());

    w.dir
        .file("matter/common/config", "[core]\n\tfsmonitor = evil\n");
    w.dir
        .file("matter/shared/hooks/post-checkout", "#!/bin/sh\n");
    w.dir.file("matter/brief.md", "");
    let paths = reported(&["common", "shared", "brief.md"]);
    let live = watch.changed(&volume, &paths, exit(), &calm());

    assert_eq!(origins(&live), [root.join("common"), root.join("shared")]);
    assert!(root.join("brief.md").is_file());
    assert!(nothing_by_path_below(&volume, &root));
}
