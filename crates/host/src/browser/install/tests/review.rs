//! The security review's findings (S27 part 3), each with the test that failed before its fix:
//! the entry-under-a-link check on names spelled two ways, a link that leaves and comes back in
//! by the unpack folder's own name, the redirect pin, and the review's cheap nits.

use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::BrowserInstallFailure;

#[cfg(unix)]
use super::browsers;
#[cfg(unix)]
use super::job;
#[cfg(unix)]
use super::support::MAC_BINARY;
use super::support::{Fake, Made, VERSION, ZipEntry, file, zip};
use super::{fails_with, run_with};
use crate::browser::chrome::Platform;
use crate::browser::install::system::{CURL_FLAGS, download_args};
use crate::browser::install::{Cancel, Failure, archive};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// `archive::check_zip` on a zip of `entries`.
fn check(entries: &[ZipEntry<'_>]) -> Result<(), Failure> {
    let dir = TempDir::new("cft-review-names");
    let path = dir.path().join("t.zip");
    write::write_file(&path, &zip(entries)).unwrap();
    archive::check_zip(&path)
}

/// A link entry `name` to `target`, as a Unix zip writes one.
fn link<'a>(name: &'a str, target: &'a [u8]) -> ZipEntry<'a> {
    ZipEntry {
        name,
        bytes: target,
        mode: Some(0o120_777),
    }
}

/// D1: `./`, `//`, a backslash or a trailing dot spell a path under a link another way; each is
/// refused, and so is a name with an empty, `.` or dot- or space-ended segment on its own.
#[test]
fn entries_under_a_link_spelled_differently_are_refused() {
    let cases: [(&str, &str); 6] = [
        ("a", "./a/evil"),
        ("x/a", "x/./a/evil"),
        ("x/a", "x//a/evil"),
        ("a", "a\\evil"),
        ("a.", "a/evil"),
        ("A", "a/evil"),
    ];
    let mut passed = Vec::new();
    for (l, f) in cases {
        if check(&[link(l, b"../../../../Library/LaunchAgents"), file(f, b"x")]).is_ok() {
            passed.push((l, f));
        }
    }
    assert_eq!(passed, Vec::<(&str, &str)>::new(), "accepted");
    for name in ["a//b", "./a", "a/./b", "a/b.", "a /b", "a\\b", "a/"] {
        let result = check(&[file(name, b"")]);
        assert_eq!(result.is_err(), name != "a/", "{name:?}");
    }
    // A link made on OS X (version made by 19) counts as a link too.
    let mut bytes = zip(&[link("a", b"/etc"), file("a/evil", b"x")]);
    let at = bytes
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
        .unwrap()
        + 5;
    bytes[at] = 19;
    let dir = TempDir::new("cft-review-osx");
    write::write_file(&dir.path().join("t.zip"), &bytes).unwrap();
    assert!(archive::check_zip(&dir.path().join("t.zip")).is_err());
}

/// A Windows 8.3 short name (`ABCDEF~1` for `abcdefghij`) spells a path under a link another
/// way: any `~` in a name is refused.
#[test]
fn a_windows_short_name_under_a_link_is_refused() {
    let entries = [link("abcdefghij", b"/etc"), file("ABCDEF~1/evil", b"x")];
    assert!(check(&entries).is_err());
    assert!(check(&[file("a~b", b"")]).is_err());
}

/// D2: a link that climbs out of the unpack folder and back in by its name (`../chrome/…`)
/// resolves inside before the rename, outside after it; the lexical rule refuses it.
#[cfg(unix)]
#[test]
fn a_link_back_in_by_the_unpack_folders_name_is_refused() {
    let dir = TempDir::new("cft-review-relink");
    let tools = Fake {
        made: vec![
            Made::File(MAC_BINARY),
            Made::Link("l", "../chrome/chrome-mac-arm64"),
        ],
        ..Fake::working()
    };
    let job = job(dir.path(), Platform::MacArm64);
    // A local folder at browsers/chrome (another tool's, or anything).
    let planted = job.app_data.join("browsers/chrome/chrome-mac-arm64");
    write::create_dir_all(&planted).unwrap();
    let (result, _) = run_with(&job, &tools, &Cancel::new());
    if let Ok(target) = &result {
        let real = dunce::canonicalize(target.join("l")).unwrap();
        let root = dunce::canonicalize(target).unwrap();
        assert!(
            real.starts_with(&root),
            "installed chrome-{VERSION}/l leads to {} outside {}",
            real.display(),
            root.display()
        );
    }
    let failure = result.unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::Unpack);
    assert!(
        failure.message.contains("climbs above"),
        "{}",
        failure.message
    );
    assert_eq!(browsers(dir.path()), ["chrome"]);
    // Climbing within the folder is fine: a link two folders down may go up one.
    let inside = Fake {
        made: vec![
            Made::File(MAC_BINARY),
            Made::File("chrome-mac-arm64/x/y/f"),
            Made::Link("chrome-mac-arm64/x/y/l", "../../x/y/f"),
        ],
        ..Fake::working()
    };
    let dir = TempDir::new("cft-review-relink-in");
    let (result, _) = run_with(
        &super::job(dir.path(), Platform::MacArm64),
        &inside,
        &Cancel::new(),
    );
    result.unwrap();
}

/// Redirects are pinned: `--max-redirs 0` (tightening the agreed 3), so a redirect fails as
/// `network`. The zip download's whole argument list, in order.
#[test]
fn the_download_pins_redirects_and_names_its_files() {
    let args = download_args(
        "https://storage.googleapis.com/z.zip",
        Path::new("/w/headers.txt"),
        Path::new("/w/chrome.zip"),
    );
    let mut expected: Vec<OsString> = ["-q", "--silent", "--show-error"]
        .into_iter()
        .chain(CURL_FLAGS)
        .map(OsString::from)
        .collect();
    for arg in ["-D", "/w/headers.txt", "-o", "/w/chrome.zip"] {
        expected.push(arg.into());
    }
    expected.push("https://storage.googleapis.com/z.zip".into());
    assert_eq!(args, expected);
    let redirs = CURL_FLAGS
        .iter()
        .position(|flag| *flag == "--max-redirs")
        .unwrap();
    assert_eq!(CURL_FLAGS.get(redirs + 1), Some(&"0"));
    assert!(CURL_FLAGS.contains(&"--location"));
}

/// Nit: a link at `browsers` is not followed by the sweep, so its target's `.partial-*` stays.
#[cfg(unix)]
#[test]
fn the_sweep_does_not_follow_a_linked_browsers_folder() {
    let dir = TempDir::new("cft-review-sweep-link");
    let elsewhere = dir.path().join("elsewhere");
    write::create_dir_all(&elsewhere.join(".partial-1-a")).unwrap();
    let browsers_link = dir.path().join("browsers");
    write::symlink(&elsewhere, &browsers_link).unwrap();
    archive::sweep_partials(&browsers_link);
    assert!(elsewhere.join(".partial-1-a").is_dir());
}

/// Nit: a file where `chrome-<version>` goes is replaced instead of failing every install.
#[test]
fn a_file_at_the_version_folders_name_is_replaced() {
    let dir = TempDir::new("cft-review-file-target");
    let browsers_dir = dir.path().join("app").join("browsers");
    write::create_dir_all(&browsers_dir).unwrap();
    write::write_file(&browsers_dir.join(format!("chrome-{VERSION}")), b"").unwrap();
    let (result, _) = run_with(
        &super::job(dir.path(), Platform::MacArm64),
        &Fake::working(),
        &Cancel::new(),
    );
    assert!(result.unwrap().join("chrome-mac-arm64").is_dir());
    // And a failure still leaves nothing new.
    let tools = Fake {
        made: vec![Made::File("chrome-mac-arm64/README")],
        ..Fake::working()
    };
    fails_with(
        "cft-review-file-target-2",
        &tools,
        BrowserInstallFailure::Unpack,
    );
}
