//! `ListPapers`: a matter's folder as it is now, sorted, with sizes and times; excluded files
//! never listed; files a checkpoint skips apart; the cap; one `FileRefId` per path, shared
//! with `RegisterFile`; an unknown matter and a missing folder.

use std::path::Path;
use std::time::{Duration, SystemTime};

use clauseo_protocol::{
    ErrorCode, FileRefId, HostQuery, MAX_PAPERS, MatterId, MatterPath, PaperSummary, QueryResult,
    SkippedFile, SkippedFileReason,
};

use crate::support::{Dir, Running, foreign_uuid, wait};
use crate::write;

/// One byte over the host's size cap (`clauseo_matters::FILE_CAP_BYTES`, 512 MB).
const OVER_CAP: u64 = 512_000_001;

/// 2020-01-01T00:00:00.250Z.
const MTIME_MS: i64 = 1_577_836_800_250;

struct Listed {
    papers: Vec<PaperSummary>,
    skipped: Vec<SkippedFile>,
    truncated: bool,
}

fn list(host: &Running, matter: MatterId) -> Listed {
    match host.query(HostQuery::ListPapers { matter }).unwrap() {
        QueryResult::Papers {
            papers,
            skipped,
            truncated,
        } => Listed {
            papers,
            skipped,
            truncated,
        },
        other => panic!("ListPapers answered {other:?}"),
    }
}

fn paths(papers: &[PaperSummary]) -> Vec<&str> {
    papers.iter().map(|paper| paper.path.as_str()).collect()
}

fn set_mtime(path: &Path) {
    let at = SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(MTIME_MS).unwrap());
    write::set_mtime(path, at);
}

fn register(host: &Running, path: &Path) -> FileRefId {
    wait("register_file", host.handle.register_file(path.to_owned())).unwrap()
}

#[test]
fn papers_are_the_folder_now_sorted_with_skipped_apart() {
    let dir = Dir::new("papers_are_the_folder_now", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "Clause 4\n")]);
    let matter = host.create_matter("m", &folder);
    // Saved since the import, never checkpointed, nested; and files never listed.
    for (rel, text) in [
        ("pleadings/drafts/claim.docx", "claim"),
        ("a.txt", "a"),
        ("Z.pdf", "pdf"),
        ("~$claim.docx", "lock"),
        ("draft.tmp", "temp"),
        (".git/HEAD", "ref"),
    ] {
        write::write(&folder.join(rel), text.as_bytes());
    }
    write::sized(&folder.join("big.bin"), OVER_CAP);
    for rel in ["brief.txt", "pleadings/drafts/claim.docx", "a.txt", "Z.pdf"] {
        set_mtime(&folder.join(rel));
    }

    let listed = list(&host, matter);
    assert_eq!(
        paths(&listed.papers),
        ["Z.pdf", "a.txt", "brief.txt", "pleadings/drafts/claim.docx"]
    );
    let sizes: Vec<u64> = listed.papers.iter().map(|paper| paper.size.get()).collect();
    assert_eq!(sizes, [3, 1, 9, 5]);
    assert!(
        listed
            .papers
            .iter()
            .all(|paper| paper.modified.as_millis() == MTIME_MS),
        "{:?}",
        listed.papers
    );
    assert_eq!(
        listed.skipped,
        [SkippedFile {
            path: MatterPath::new("big.bin").unwrap(),
            reason: SkippedFileReason::OverSizeCap,
        }]
    );
    assert!(!listed.truncated);
    host.shutdown();
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_a_paper() {
    let dir = Dir::new("an_unreadable_file_is_a_paper", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    // After the import, which would fail reading it: listing only stats.
    write::write(&folder.join("locked.docx"), b"locked");
    write::set_mode(&folder.join("locked.docx"), 0o000);
    let listed = list(&host, matter);
    write::set_mode(&folder.join("locked.docx"), 0o644);
    assert_eq!(paths(&listed.papers), ["brief.txt", "locked.docx"]);
    assert_eq!(listed.skipped, []);
    host.shutdown();
}

#[test]
fn a_folder_over_the_cap_lists_the_first_files_and_says_truncated() {
    let dir = Dir::new("a_folder_over_the_cap", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[]);
    let matter = host.create_matter("m", &folder);
    let cap = usize::try_from(MAX_PAPERS).unwrap();
    for n in 0..=cap {
        write::write(&folder.join(format!("{}/doc-{n:05}.txt", n % 7)), b"x");
    }
    let listed = list(&host, matter);
    assert_eq!(listed.papers.len(), cap);
    assert!(listed.truncated);
    assert!(
        listed
            .papers
            .windows(2)
            .all(|pair| pair[0].path.as_str() < pair[1].path.as_str()),
        "sorted by path"
    );
    // The one left out sorts last.
    let last = format!(
        "6/doc-{:05}.txt",
        (0..=cap).filter(|n| n % 7 == 6).max().unwrap()
    );
    assert!(
        !paths(&listed.papers).contains(&last.as_str()),
        "{last} listed"
    );
    host.shutdown();
}

#[test]
fn a_path_keeps_its_file_ref_and_shares_it_with_register_file() {
    let dir = Dir::new("a_path_keeps_its_file_ref", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[("brief.txt", "x\n"), ("sub/note.txt", "n\n")]);
    let matter = host.create_matter("m", &folder);
    let first = list(&host, matter).papers;
    // Changed since, and a file added before it: ids follow the path, not the content.
    write::write(&folder.join("brief.txt"), b"changed\n");
    write::write(&folder.join("a.txt"), b"a\n");
    let second = list(&host, matter).papers;
    let id = |papers: &[PaperSummary], at: &str| {
        papers
            .iter()
            .find(|paper| paper.path.as_str() == at)
            .map_or_else(|| panic!("{at} not listed"), |paper| paper.file)
    };
    assert_eq!(id(&first, "brief.txt"), id(&second, "brief.txt"));
    assert_eq!(id(&first, "sub/note.txt"), id(&second, "sub/note.txt"));
    assert_ne!(id(&second, "a.txt"), id(&second, "brief.txt"));
    // RegisterFile gives a listed path the same id, and its own id twice.
    let note = folder.join("sub").join("note.txt");
    assert_eq!(register(&host, &note), id(&first, "sub/note.txt"));
    let elsewhere = dir.folder("other", &[("x.txt", "x\n")]).join("x.txt");
    assert_eq!(register(&host, &elsewhere), register(&host, &elsewhere));
    assert_ne!(register(&host, &elsewhere), id(&first, "brief.txt"));
    host.shutdown();
}

#[test]
fn an_unknown_matter_and_a_missing_folder_are_refused() {
    let dir = Dir::new("papers_unknown_and_missing", &[]);
    let host = dir.start(0);
    let unknown = MatterId::new(foreign_uuid(1)).unwrap();
    let error = host
        .query(HostQuery::ListPapers { matter: unknown })
        .unwrap_err();
    assert_eq!(error.code(), ErrorCode::CommandUnknownMatter, "{error}");

    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    assert_eq!(paths(&list(&host, matter).papers), ["brief.txt"]);
    write::rename(&folder, &folder.with_file_name("moved"));
    let error = host.query(HostQuery::ListPapers { matter }).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing, "{error}");
    host.shutdown();
}
