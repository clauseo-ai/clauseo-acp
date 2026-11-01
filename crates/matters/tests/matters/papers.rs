//! Listing a matter's papers: the folder as it is now, by the checkpoint walk's rules, sorted,
//! capped, with the files a checkpoint skips apart; an unreadable file and subfolder; a missing
//! folder.

use std::time::{Duration, SystemTime};

use clauseo_matters::{Limits, Paper, PaperList};
use clauseo_protocol::{ErrorCode, SkippedFile, SkippedFileReason, Timestamp};

use crate::fixture::{Fixture, path};
use crate::support;

fn paper(at: &str, size: u64, millis: i64) -> Paper {
    Paper {
        path: path(at),
        size,
        modified: Timestamp::new(millis).unwrap(),
    }
}

fn skipped(at: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: path(at),
        reason,
    }
}

fn paths(papers: &PaperList) -> Vec<&str> {
    papers
        .papers
        .iter()
        .map(|paper| paper.path.as_str())
        .collect()
}

/// 2020-01-01T00:00:00.250Z.
const MTIME_MS: i64 = 1_577_836_800_250;

fn set_mtime(fx: &Fixture, rel: &str) {
    let at = SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(MTIME_MS).unwrap());
    support::set_mtime(fx.file(rel), at);
}

#[test]
fn papers_are_the_folder_now_sorted_with_skipped_apart() {
    let mut fx = Fixture::new("papers-sorted");
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx.write("b.txt", "bee");
    let matter = fx.import();
    // Saved since the import, never checkpointed: listed all the same.
    for (rel, text) in [
        ("a.txt", "a"),
        ("a/z.docx", "zed"),
        ("a0.txt", "a zero"),
        ("Z.pdf", "pdf"),
        ("pleadings/drafts/claim.docx", "claim"),
    ] {
        fx.write(rel, text);
        set_mtime(&fx, rel);
    }
    set_mtime(&fx, "b.txt");
    // Never listed: excluded names and folders.
    for rel in [
        "~$claim.docx",
        "x.tmp",
        ".DS_Store",
        ".git/HEAD",
        "dir.tmp/kept.txt",
    ] {
        fx.write(rel, "x");
    }
    set_mtime(&fx, "dir.tmp/kept.txt");
    // Skipped: over the cap, and cloud-only.
    fx.write("big.bin", [7; 20]);
    fx.write("cloud.docx", "cloud");
    fx.probe.mark(fx.file("cloud.docx"));

    let papers = matter.papers(100).unwrap();
    assert_eq!(
        papers.papers,
        [
            paper("Z.pdf", 3, MTIME_MS),
            paper("a.txt", 1, MTIME_MS),
            paper("a/z.docx", 3, MTIME_MS),
            paper("a0.txt", 6, MTIME_MS),
            paper("b.txt", 3, MTIME_MS),
            paper("dir.tmp/kept.txt", 1, MTIME_MS),
            paper("pleadings/drafts/claim.docx", 5, MTIME_MS),
        ]
    );
    assert_eq!(
        papers.skipped,
        [
            skipped("big.bin", SkippedFileReason::OverSizeCap),
            skipped("cloud.docx", SkippedFileReason::CloudOnly),
        ]
    );
    assert!(!papers.truncated);
}

#[test]
fn the_cap_counts_papers_and_skipped_together() {
    let mut fx = Fixture::new("papers-cap");
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx.write("a.txt", "x");
    let matter = fx.import();
    for rel in ["a.txt", "b/c.txt", "d.txt", "e/f/g.txt"] {
        fx.write(rel, "x");
    }
    fx.write("c.bin", [7; 20]);

    let all = matter.papers(5).unwrap();
    assert_eq!(paths(&all), ["a.txt", "b/c.txt", "d.txt", "e/f/g.txt"]);
    assert!(!all.truncated, "exactly at the limit is not truncated");

    let first = matter.papers(3).unwrap();
    assert_eq!(paths(&first), ["a.txt", "b/c.txt"]);
    assert_eq!(
        first.skipped,
        [skipped("c.bin", SkippedFileReason::OverSizeCap)]
    );
    assert!(first.truncated);

    let none = matter.papers(0).unwrap();
    assert!(none.papers.is_empty() && none.skipped.is_empty());
    assert!(none.truncated);
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_a_paper_and_an_unreadable_folder_is_left_out() {
    let fx = Fixture::new("papers-unreadable");
    fx.write("open.docx", "open");
    let matter = fx.import();
    fx.write("locked.docx", "locked");
    fx.write("shut/inside.docx", "inside");
    support::set_mode(fx.file("locked.docx"), 0o000);
    support::set_mode(fx.file("shut"), 0o000);
    // Root reads a folder whatever its mode: then only the file half of the test holds.
    let shut = std::fs::read_dir(fx.file("shut")).is_err();

    let papers = matter.papers(100).unwrap();
    support::set_mode(fx.file("shut"), 0o755);
    support::set_mode(fx.file("locked.docx"), 0o644);
    if shut {
        assert_eq!(paths(&papers), ["locked.docx", "open.docx"]);
    } else {
        assert_eq!(
            paths(&papers),
            ["locked.docx", "open.docx", "shut/inside.docx"]
        );
    }
    assert_eq!(papers.skipped, []);
}

#[test]
fn a_missing_folder_is_folder_missing() {
    let fx = Fixture::new("papers-missing");
    fx.write("a.txt", "a");
    let matter = fx.import();
    support::rename(&fx.folder, fx.dir.path().join("moved"));
    let err = matter.papers(10).unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFolderMissing, "{err:?}");
}
