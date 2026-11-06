//! The download mover: names, the no-replace move, Chrome's in-progress names and the OS marks.

use std::path::{Path, PathBuf};

use super::{
    BLOCKED_NOTE, Mover, Sweeper, in_progress, is_protected, marks, place, safe_name, sweep,
};
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

/// Whether this OS has a download mark the mover waits for.
const MARKS: bool = cfg!(any(windows, target_os = "macos"));

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

/// A finished file without its OS mark waits up to the given number of sweeps for Chrome's
/// (Windows and macOS), then is marked in staging and moves; on Linux, which has no mark, it moves
/// at once and nothing else changes.
#[test]
fn an_unmarked_file_waits_for_chromes_mark_then_moves_marked() {
    let (dir, staging, downloads) = scratch("unmarked");
    write::write(&staging.join("report.pdf"), "download").unwrap();
    let mut sweeper = Sweeper::new(staging.clone(), downloads.clone(), 2);
    let waits = if MARKS { 2 } else { 0 };
    for _ in 0..waits {
        sweeper.sweep();
        assert_eq!(names(&staging), ["report.pdf"]);
        assert_eq!(names(&downloads), NONE);
    }
    sweeper.sweep();
    assert_eq!(names(&staging), NONE);
    assert_eq!(names(&downloads), ["report.pdf"]);
    assert_eq!(sweeper.waited.len(), 0);
    let moved = downloads.join("report.pdf");
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "download");
    assert!(marks::marked(&moved));
    assert_eq!(marks::read(&moved).is_some(), MARKS);
    write::remove_dir_all(&dir).unwrap();
}

/// A file Chrome has marked moves on the next sweep, however long the wait: the wait is only for
/// files without a mark, and a slow one holds back no other.
#[test]
fn a_marked_file_moves_while_an_unmarked_one_waits() {
    let (dir, staging, downloads) = scratch("one-waits");
    write::write(&staging.join("slow.pdf"), "a").unwrap();
    write::write(&staging.join("done.pdf"), "b").unwrap();
    marks::ensure(&staging.join("done.pdf")).unwrap();
    let mut sweeper = Sweeper::new(staging.clone(), downloads.clone(), 150);
    sweeper.sweep();
    if MARKS {
        assert_eq!(names(&staging), ["slow.pdf"]);
        assert_eq!(names(&downloads), ["done.pdf"]);
    } else {
        assert_eq!(names(&downloads), ["done.pdf", "slow.pdf"]);
    }
    write::remove_dir_all(&dir).unwrap();
}

/// Fail closed: where the probe cannot write a mark, no download reaches the downloads folder;
/// files stay in staging, and the folder gets one note, written once and never replaced.
#[test]
fn downloads_are_held_back_where_no_mark_can_be_written() {
    let (dir, staging, downloads) = scratch("blocked");
    write::write(&staging.join("report.pdf"), "download").unwrap();
    let probe = |_: &Path| -> std::io::Result<()> {
        Err(std::io::Error::other("no streams on this drive"))
    };
    let mut sweeper = Sweeper::with_probe(staging.clone(), downloads.clone(), 0, probe);
    sweeper.sweep();
    write::write(&staging.join("second.pdf"), "download").unwrap();
    sweeper.sweep();
    assert_eq!(names(&staging), ["report.pdf", "second.pdf"]);
    assert_eq!(names(&downloads), [BLOCKED_NOTE]);
    let note = std::fs::read_to_string(downloads.join(BLOCKED_NOTE)).unwrap();
    assert!(
        note.contains("\"downloaded from the internet\" mark"),
        "{note}"
    );
    assert_eq!(sweeper.marks_work, Some(false));
    write::remove_dir_all(&dir).unwrap();
}

/// The real probe on this OS's temp folder (NTFS, APFS or a Linux file system): it passes and
/// leaves nothing behind.
#[test]
fn the_probe_passes_here_and_leaves_nothing() {
    let (dir, staging, _) = scratch("probe");
    marks::probe(&staging).unwrap();
    assert_eq!(names(&staging), NONE);
    write::remove_dir_all(&dir).unwrap();
}

/// The `ZoneId` rules: 1 to 4 kept, 0 or a value that does not read raised to 3.
#[test]
fn zone_ids_below_the_internet_are_kept_only_for_the_firms_own_zones() {
    for (text, zone) in [
        (
            "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://a.test/\r\n",
            Some(3),
        ),
        ("[ZoneTransfer]\nZoneId= 2 \n", Some(2)),
        ("[ZoneTransfer]\r\nZoneId=0\r\n", Some(0)),
        ("[ZoneTransfer]\r\nZoneId=x\r\n", None),
        ("[ZoneTransfer]\r\n", None),
        ("", None),
    ] {
        assert_eq!(marks::zone_of(text), zone, "{text:?}");
    }
    for (zone, kept) in [
        (Some(0), false),
        (Some(1), true),
        (Some(2), true),
        (Some(3), true),
        (Some(4), true),
        (Some(5), false),
        (None, false),
    ] {
        assert_eq!(marks::zone_kept(zone), kept, "{zone:?}");
    }
}

#[test]
fn the_quarantine_value_has_the_attributes_format() {
    assert_eq!(
        marks::quarantine_value(0x6543_2100),
        "0081;65432100;Clauseo;"
    );
}

/// Windows: a moved file has a `Zone.Identifier`: Chrome's own untouched when it names zone 1
/// to 4, and `ZoneId=3` when the file had none, zone 0, or one that does not read.
#[cfg(windows)]
#[test]
fn a_moved_file_has_its_zone_with_or_without_a_mark_before() {
    let (dir, staging, downloads) = scratch("zone");
    let chromes = "[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/a.pdf\r\n";
    let trusted = "[ZoneTransfer]\r\nZoneId=2\r\n";
    for (name, stream) in [
        ("marked.pdf", Some(chromes)),
        ("trusted.pdf", Some(trusted)),
        ("local.pdf", Some("[ZoneTransfer]\r\nZoneId=0\r\n")),
        ("garbled.pdf", Some("ZoneId=three")),
        ("bare.pdf", None),
    ] {
        write::write(&staging.join(name), name).unwrap();
        if let Some(stream) = stream {
            write::write(&marks::stream_of(&staging.join(name)), stream).unwrap();
        }
    }
    sweep(&staging, &downloads);
    let read = |name: &str| marks::read(&downloads.join(name));
    assert_eq!(read("marked.pdf").as_deref(), Some(chromes));
    assert_eq!(read("trusted.pdf").as_deref(), Some(trusted));
    for raised in ["local.pdf", "garbled.pdf", "bare.pdf"] {
        assert_eq!(read(raised).as_deref(), Some(marks::ZONE_MARK), "{raised}");
    }
    assert!(marks::ZONE_MARK.contains("ZoneId=3"));
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

/// macOS: a moved file has a `com.apple.quarantine` attribute: the one it had, untouched, or the
/// guard's when it had none.
#[cfg(target_os = "macos")]
#[test]
fn a_moved_file_is_quarantined_with_or_without_a_mark_before() {
    let (dir, staging, downloads) = scratch("quarantine");
    let chromes = "0083;6543210f;Google Chrome;";
    write::write(&staging.join("marked.pdf"), "a").unwrap();
    write::set_xattr(
        &staging.join("marked.pdf"),
        c"com.apple.quarantine",
        chromes.as_bytes(),
    )
    .unwrap();
    write::write(&staging.join("bare.pdf"), "b").unwrap();
    sweep(&staging, &downloads);
    assert_eq!(names(&downloads), ["bare.pdf", "marked.pdf"]);
    assert_eq!(
        marks::read(&downloads.join("marked.pdf")).as_deref(),
        Some(chromes)
    );
    let ours = marks::read(&downloads.join("bare.pdf")).unwrap();
    assert!(
        ours.starts_with("0081;") && ours.ends_with(";Clauseo;"),
        "{ours}"
    );
    write::remove_dir_all(&dir).unwrap();
}

/// Lazy start's idle stop drops Chrome, whose mover then sweeps once without waiting for a mark:
/// what finished is delivered, marked, and staging keeps only the unfinished file. The next
/// start's mover, on the same kept staging folder, delivers again.
#[test]
fn an_idle_stop_delivers_what_finished_and_the_next_start_delivers_again() {
    let (dir, staging, downloads) = scratch("idle-stop");
    let mover = Mover::start(staging.clone(), downloads.clone());
    write::write(&staging.join("report.pdf"), "first").unwrap();
    write::write(&staging.join("big.zip.crdownload"), "half").unwrap();
    mover.finish();
    assert_eq!(names(&downloads), ["report.pdf"]);
    assert_eq!(names(&staging), ["big.zip.crdownload"]);
    assert!(marks::marked(&downloads.join("report.pdf")));
    let mover = Mover::start(staging.clone(), downloads.clone());
    write::write(&staging.join("second.pdf"), "second").unwrap();
    mover.finish();
    assert_eq!(names(&downloads), ["report.pdf", "second.pdf"]);
    write::remove_dir_all(&dir).unwrap();
}
