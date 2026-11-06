//! The download mover: names, the no-replace move and Chrome's in-progress names.

use std::path::{Path, PathBuf};

use super::{in_progress, is_protected, place, safe_name, sweep};
use crate::write;

/// A fresh `<temp>/clauseo-mcp-downloads-<test>-<pid>` with empty `staging` and `downloads`.
fn scratch(test: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "clauseo-mcp-downloads-{test}-{}",
        std::process::id()
    ));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    let (staging, downloads) = (dir.join("staging"), dir.join("downloads"));
    write::create_dir_all(&staging).unwrap();
    write::create_dir_all(&downloads).unwrap();
    (dir, staging, downloads)
}

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// No names, for an empty folder.
const NONE: [&str; 0] = [];

/// A file that appears under the chosen name between the mover's look and its move is kept,
/// and the download takes the next free name; so does one that appears there too.
#[test]
fn a_file_that_appears_at_the_target_is_not_replaced() {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-place-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    let (staging, downloads) = (dir.join("staging"), dir.join("downloads"));
    write::create_dir_all(&staging).unwrap();
    write::create_dir_all(&downloads).unwrap();
    let from = staging.join("report.pdf");
    write::write(&from, "download").unwrap();
    let mut raced = 0;
    let placed = place(&from, &downloads, "report.pdf", |target: &Path| {
        if raced < 2 {
            raced += 1;
            write::write(target, "the agent's own").unwrap();
        }
    })
    .unwrap();
    assert_eq!(placed, downloads.join("report (3).pdf"));
    let read = |name: &str| std::fs::read_to_string(downloads.join(name)).unwrap();
    assert_eq!(read("report.pdf"), "the agent's own");
    assert_eq!(read("report (2).pdf"), "the agent's own");
    assert_eq!(read("report (3).pdf"), "download");
    assert!(!from.exists());
    write::remove_dir_all(&dir).unwrap();
}

#[test]
fn protected_names_are_renamed_in_any_case() {
    for (name, safe) in [
        ("CLAUDE.md", "CLAUDE (download).md"),
        ("claude.MD", "claude (download).MD"),
        (".mcp.json", ".mcp (download).json"),
        ("AGENTS.md", "AGENTS (download).md"),
        ("agents.md. ", "agents (download).md"),
        (".claude", ".claude (download)"),
        (".Codex", ".Codex (download)"),
        ("CLAUDE.local.md", "CLAUDE.local (download).md"),
        ("AGENT\u{17f}.md", "AGENT\u{17f} (download).md"),
    ] {
        assert!(is_protected(name), "{name}");
        assert_eq!(safe_name(name), safe);
        assert!(!is_protected(&safe_name(name)), "{safe}");
    }
}

#[test]
fn other_names_are_kept() {
    for name in [
        "report.pdf",
        "CLAUDE.md.txt",
        "my CLAUDE.md",
        ".mcp",
        "x.crdownload",
    ] {
        assert!(!is_protected(name), "{name}");
        assert_eq!(safe_name(name), name);
    }
}

/// G17's Windows run: Chrome's first name for a download on Windows is `<guid>.tmp`
/// (`file_util_win.cc`), which the mover must leave alone; any other `.tmp` is the user's.
#[test]
fn chromes_in_progress_names_are_recognised_on_every_os() {
    for name in [
        "9bfaa860-c93d-4921-a7ba-75f845a80d7f.tmp",
        "69CEEAF7-D73A-4DA4-8443-5B647A8D5A79.tmp",
        "report.pdf.crdownload",
        "Unconfirmed 123456.crdownload",
        ".com.google.Chrome.a1B2c3",
        ".org.chromium.Chromium.a1B2c3",
    ] {
        assert!(in_progress(name), "{name}");
    }
    for name in [
        "notes.tmp",
        ".tmp",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7.tmp",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7f0.tmp",
        "9bfaa860c93d4921a7ba75f845a80d7f.tmp",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7g.tmp",
        "x9bfaa860-c93d-4921-a7ba-75f845a80d7f.tmp",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7f.tmp.pdf",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7f.TMP.txt",
        "9bfaa860-c93d-4921-a7ba-75f845a80d7f",
    ] {
        assert!(!in_progress(name), "{name}");
    }
}

/// A half-written `<guid>.tmp` stays in the staging folder however many sweeps pass, and the
/// file moves once Chrome renames it to its final name.
#[test]
fn a_half_written_guid_tmp_stays_until_chrome_renames_it() {
    let (dir, staging, downloads) = scratch("guid-tmp");
    let partial = staging.join("9bfaa860-c93d-4921-a7ba-75f845a80d7f.tmp");
    write::write(&partial, "half of it").unwrap();
    for _ in 0..3 {
        sweep(&staging, &downloads);
    }
    assert_eq!(
        names(&staging),
        ["9bfaa860-c93d-4921-a7ba-75f845a80d7f.tmp"]
    );
    assert_eq!(names(&downloads), NONE);
    write::move_new(&partial, &staging.join("report.pdf")).unwrap();
    sweep(&staging, &downloads);
    assert_eq!(names(&staging), NONE);
    assert_eq!(names(&downloads), ["report.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}

/// A `.tmp` that is no GUID is a finished download, and moves.
#[test]
fn a_tmp_that_is_no_guid_moves() {
    let (dir, staging, downloads) = scratch("plain-tmp");
    write::write(&staging.join("notes.tmp"), "the user's").unwrap();
    sweep(&staging, &downloads);
    assert_eq!(names(&staging), NONE);
    assert_eq!(names(&downloads), ["notes.tmp"]);
    write::remove_dir_all(&dir).unwrap();
}

/// Windows: a file held open without delete sharing (Chrome, or a scanner) cannot be moved; it
/// stays in the staging folder, and the next sweep after it is closed moves it.
#[cfg(windows)]
#[test]
fn a_file_held_open_is_moved_by_a_later_sweep() {
    let (dir, staging, downloads) = scratch("held");
    let file = staging.join("report.pdf");
    write::write(&file, "download").unwrap();
    let held = write::open_unshared(&file).unwrap();
    sweep(&staging, &downloads);
    assert_eq!(names(&staging), ["report.pdf"]);
    assert_eq!(names(&downloads), NONE);
    drop(held);
    sweep(&staging, &downloads);
    assert_eq!(names(&staging), NONE);
    assert_eq!(names(&downloads), ["report.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}
