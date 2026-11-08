//! Links in the zip (stage F's defence in depth): none at all in a zip for Windows, an escaping
//! link refused from the zip through the unpack to the walk, an entry written through an earlier
//! link refused, and a macOS framework's `Versions/Current -> A` taken and made again.
//!
//! The end-to-end tests unpack with the system's `/usr/bin/unzip` when it is there (Linux and
//! macOS developers, CI's Ubuntu) and say so and stop when it is not; the walk's half of the
//! framework test builds the tree by hand and always runs.

#[cfg(unix)]
use std::path::Path;

use clauseo_protocol::BrowserInstallFailure;

#[cfg(unix)]
use super::fails_with;
use super::support::{Fake, ZipEntry, file, zip};
#[cfg(unix)]
use super::support::{MAC_BINARY, UNZIP};
use super::{browsers, job, run_with};
use crate::browser::chrome::Platform;
use crate::browser::install::{Cancel, archive};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A link entry `name` to `target`, as a Unix zip writes one.
fn link<'a>(name: &'a str, target: &'a [u8]) -> ZipEntry<'a> {
    ZipEntry {
        name,
        bytes: target,
        mode: Some(0o120_777),
    }
}

/// `archive::check_zip` on a zip of `entries`, links allowed or not.
fn check(
    entries: &[ZipEntry<'_>],
    links_allowed: bool,
) -> Result<(), crate::browser::install::Failure> {
    let dir = TempDir::new("cft-links-check");
    let path = dir.path().join("t.zip");
    write::write_file(&path, &zip(entries)).unwrap();
    archive::check_zip(&path, links_allowed)
}

/// Whether the system's unzip is there; says so when it is not.
#[cfg(unix)]
fn have_unzip() -> bool {
    let there = Path::new(UNZIP).is_file();
    if !there {
        println!("no {UNZIP}: the end-to-end half of this test is skipped");
    }
    there
}

/// The framework of a macOS app bundle, inside the Apple silicon folder.
#[cfg(unix)]
const FRAMEWORK: &str = "chrome-mac-arm64/Foo.framework";

#[test]
fn a_zip_for_windows_with_any_link_entry_is_refused_before_unpacking() {
    assert!(!archive::links_allowed(Platform::WindowsX64));
    assert!(!archive::links_allowed(Platform::WindowsArm64));
    assert!(archive::links_allowed(Platform::MacArm64));
    assert!(archive::links_allowed(Platform::MacX64));
    // Even a link that stays inside, with nothing under it.
    let inside = [
        file("chrome-win64/chrome.exe", b"x"),
        link("chrome-win64/l", b"chrome.exe"),
    ];
    let failure = check(&inside, false).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::Unpack);
    assert!(failure.message.contains("link"), "{}", failure.message);
    check(&inside, true).unwrap();
    check(&[file("chrome-win64/chrome.exe", b"x")], false).unwrap();
    // Through the download: refused before the unpack tool runs, and nothing is left.
    let tools = Fake {
        zip: zip(&inside),
        ..Fake::working()
    };
    let dir = TempDir::new("cft-links-windows");
    let (result, _) = run_with(
        &job(dir.path(), Platform::WindowsX64),
        &tools,
        &Cancel::new(),
    );
    assert_eq!(result.unwrap_err().code, BrowserInstallFailure::Unpack);
    assert_eq!(tools.calls(), ["download"]);
    assert_eq!(browsers(dir.path()), Vec::<String>::new());
}

#[test]
fn an_entry_written_through_an_earlier_link_is_refused() {
    // The plain case, whatever the link's target: outside, absolute, or inside.
    for target in [&b"../../outside"[..], b"/etc", b"y"] {
        let entries = [link("x/l", target), file("x/y/f", b""), file("x/l/f", b"x")];
        let failure = check(&entries, true).unwrap_err();
        assert!(
            failure.message.contains("under the link"),
            "{}",
            failure.message
        );
    }
}

#[cfg(unix)]
#[test]
fn a_link_escaping_the_folder_is_refused_from_the_zip_through_the_walk() {
    let entries = [
        file(MAC_BINARY, b"chrome"),
        link("chrome-mac-arm64/out", b"../../../outside"),
    ];
    // The names pass: the link's target is the walk's to judge.
    check(&entries, true).unwrap();
    if !have_unzip() {
        return;
    }
    let tools = Fake {
        zip: zip(&entries),
        unzip: true,
        ..Fake::working()
    };
    let failure = fails_with("cft-links-escape", &tools, BrowserInstallFailure::Unpack);
    assert!(
        failure.message.contains("climbs above"),
        "{}",
        failure.message
    );
    assert_eq!(tools.calls(), ["download", "unpack"]);
}

#[cfg(unix)]
#[test]
fn a_frameworks_current_link_passes_and_is_made_again() {
    let a_binary = format!("{FRAMEWORK}/Versions/A/Foo");
    let a_resource = format!("{FRAMEWORK}/Versions/A/Resources/r.txt");
    let current = format!("{FRAMEWORK}/Versions/Current");
    let top = format!("{FRAMEWORK}/Foo");
    let entries = [
        file(MAC_BINARY, b"chrome"),
        file(&a_binary, b"foo"),
        file(&a_resource, b"r"),
        link(&current, b"A"),
        link(&top, b"Versions/Current/Foo"),
    ];
    check(&entries, true).unwrap();
    // The walk's half, the tree made by hand.
    let dir = TempDir::new("cft-links-framework-hand");
    let root = dir.path().join("chrome");
    for name in [&a_binary, &a_resource] {
        let path = root.join(name);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    write::symlink(Path::new("A"), &root.join(&current)).unwrap();
    write::symlink(Path::new("Versions/Current/Foo"), &root.join(&top)).unwrap();
    archive::check_tree(&root).unwrap();
    if !have_unzip() {
        return;
    }
    // From the zip, through unzip and both walks, into place.
    let tools = Fake {
        zip: zip(&entries),
        unzip: true,
        ..Fake::working()
    };
    let dir = TempDir::new("cft-links-framework");
    let (result, _) = run_with(&job(dir.path(), Platform::MacArm64), &tools, &Cancel::new());
    let target = result.unwrap();
    assert_eq!(
        std::fs::read_link(target.join(&current)).unwrap(),
        Path::new("A")
    );
    assert_eq!(std::fs::read(target.join(&top)).unwrap(), b"foo");
}
