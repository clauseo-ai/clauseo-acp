//! The install record: what it holds, and what the check at launch refuses.

use super::{Entry, Kind, Refusal, read, record_path, verify, write as write_record};
use crate::pipeline::tests::support::TempDir;
use crate::write;

const ZIP_SHA: &str = "8cdeb344e9b2cefe59f4aea8d1d93d5d93054c03cde217665c16fc1f62e72a66";

/// A small tree under `dir/app/browsers/chrome-1.0`: two files in a folder; returns the folder.
fn tree(dir: &std::path::Path) -> std::path::PathBuf {
    let folder = dir.join("app/browsers/chrome-1.0");
    write::create_dir_all(&folder.join("chrome-win64/locales")).unwrap();
    write::write_file(&folder.join("chrome-win64/chrome.exe"), b"abc").unwrap();
    write::write_file(&folder.join("chrome-win64/locales/en.pak"), b"").unwrap();
    folder
}

#[test]
fn the_record_lists_every_entry_sorted_with_size_mode_and_sha256() {
    let dir = TempDir::new("record-holds");
    let app = dir.path().join("app");
    let folder = tree(dir.path());
    write_record(&app, "win64", "1.0", ZIP_SHA, &folder).unwrap();
    assert!(record_path(&app).starts_with(app.join("browser-pins")));
    let record = read(&app).unwrap();
    assert_eq!(record.version, "1.0");
    assert_eq!(record.platform, "win64");
    assert_eq!(record.zip_sha256, ZIP_SHA);
    let paths: Vec<&str> = record
        .tree
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "chrome-win64",
            "chrome-win64/chrome.exe",
            "chrome-win64/locales",
            "chrome-win64/locales/en.pak"
        ]
    );
    let exe: &Entry = &record.tree[1];
    assert_eq!(exe.kind, Kind::File);
    assert_eq!(exe.size, 3);
    // sha256("abc").
    assert_eq!(
        exe.sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(record.tree[0].kind, Kind::Dir);
    assert_eq!(record.tree[0].sha256, "");
    assert_eq!(verify(&app, "win64", "1.0", &folder), Ok(()));
}

#[test]
fn the_check_refuses_another_version_platform_or_a_missing_record() {
    let dir = TempDir::new("record-other");
    let app = dir.path().join("app");
    let folder = tree(dir.path());
    assert_eq!(verify(&app, "win64", "1.0", &folder), Err(Refusal::Missing));
    write_record(&app, "win64", "1.0", ZIP_SHA, &folder).unwrap();
    assert_eq!(
        verify(&app, "win64", "1.0.0", &folder),
        Err(Refusal::OtherVersion)
    );
    assert_eq!(
        verify(&app, "mac-x64", "1.0", &folder),
        Err(Refusal::OtherVersion)
    );
    write::write_file(&record_path(&app), br#"{"schema":2}"#).unwrap();
    assert_eq!(
        verify(&app, "win64", "1.0", &folder),
        Err(Refusal::Unreadable)
    );
}

#[test]
fn the_check_refuses_a_changed_listing_before_any_hash() {
    let dir = TempDir::new("record-listing");
    let app = dir.path().join("app");
    let folder = tree(dir.path());
    write_record(&app, "win64", "1.0", ZIP_SHA, &folder).unwrap();
    // An empty folder added is an entry too.
    write::create_dir_all(&folder.join("chrome-win64/extra")).unwrap();
    assert_eq!(verify(&app, "win64", "1.0", &folder), Err(Refusal::Listing));
    write::remove_dir_all(&folder.join("chrome-win64/extra")).unwrap();
    assert_eq!(verify(&app, "win64", "1.0", &folder), Ok(()));
    write::write_file(&folder.join("chrome-win64/chrome.exe"), b"abz").unwrap();
    assert_eq!(verify(&app, "win64", "1.0", &folder), Err(Refusal::Content));
}

/// A link is recorded by its target as text, never followed; another target is refused.
#[cfg(unix)]
#[test]
fn links_are_recorded_by_target() {
    let dir = TempDir::new("record-links");
    let app = dir.path().join("app");
    let folder = tree(dir.path());
    write::symlink(
        std::path::Path::new("locales"),
        &folder.join("chrome-win64/Current"),
    )
    .unwrap();
    write_record(&app, "mac-arm64", "1.0", ZIP_SHA, &folder).unwrap();
    let record = read(&app).unwrap();
    let link = record
        .tree
        .iter()
        .find(|entry| entry.kind == Kind::Link)
        .unwrap();
    assert_eq!(link.target, "locales");
    assert_eq!(verify(&app, "mac-arm64", "1.0", &folder), Ok(()));
    write::remove_file(&folder.join("chrome-win64/Current")).unwrap();
    write::symlink(
        std::path::Path::new("chrome.exe"),
        &folder.join("chrome-win64/Current"),
    )
    .unwrap();
    assert_eq!(
        verify(&app, "mac-arm64", "1.0", &folder),
        Err(Refusal::Listing)
    );
}

/// Times the check on a real unpacked Chrome for Testing tree, for record 0029's budget: set
/// `CLAUSEO_RECORD_TIMING` to a folder holding one (such as `chrome-linux64/` from Chrome for
/// Testing's zip) and run with `--ignored --nocapture` in a release build.
#[test]
#[ignore = "needs a real Chrome for Testing tree; run by hand"]
fn timing_on_a_real_tree() {
    use crate::clock::{Clock as _, SystemClock};
    let Some(tree) = std::env::var_os("CLAUSEO_RECORD_TIMING") else {
        return;
    };
    let tree = std::path::PathBuf::from(tree);
    let app = TempDir::new("record-timing");
    let mono = || SystemClock.now().mono;
    let started = mono();
    write_record(app.path(), "linux64", "1.0", ZIP_SHA, &tree).unwrap();
    let written = mono().saturating_sub(started);
    let mut runs = Vec::new();
    for _ in 0..5 {
        let started = mono();
        assert_eq!(verify(app.path(), "linux64", "1.0", &tree), Ok(()));
        runs.push(mono().saturating_sub(started));
    }
    let entries = read(app.path()).unwrap().tree.len();
    println!("entries {entries}; write {written:?}; verify runs {runs:?}");
}
