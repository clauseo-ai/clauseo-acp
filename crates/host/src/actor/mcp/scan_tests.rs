//! The scan's skip: nothing is read when the list and every file's stamp are as the last fresh
//! scan saw them; any change, or a finder that knows no files, runs the finder again.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use super::{LastScan, ScanAnswer, scan_job};
use crate::config::mcp::{McpFileStamp, McpFound, McpImportFinder};

/// A finder that sees one file, whose stamp a test sets, and counts its runs.
struct OneFile {
    stamp: Mutex<McpFileStamp>,
    runs: AtomicUsize,
    knows_files: bool,
}

impl OneFile {
    fn new(knows_files: bool) -> Self {
        Self {
            stamp: Mutex::new(Some((SystemTime::UNIX_EPOCH, 10))),
            runs: AtomicUsize::new(0),
            knows_files,
        }
    }

    fn touch(&self) {
        *self.stamp.lock().unwrap() = Some((SystemTime::UNIX_EPOCH + Duration::from_secs(1), 10));
    }
}

fn file() -> PathBuf {
    Path::new("/Users/ana/.claude.json").to_owned()
}

impl McpImportFinder for OneFile {
    fn find(&self, _: &[String]) -> McpFound {
        self.runs.fetch_add(1, Ordering::SeqCst);
        let files = if self.knows_files {
            vec![(file(), *self.stamp.lock().unwrap())]
        } else {
            Vec::new()
        };
        McpFound {
            files,
            ..McpFound::default()
        }
    }

    fn stamps(&self, paths: &[PathBuf]) -> Vec<McpFileStamp> {
        paths.iter().map(|_| *self.stamp.lock().unwrap()).collect()
    }
}

/// A fresh scan's `LastScan`; panics on `Unchanged`.
fn fresh(answer: ScanAnswer) -> LastScan {
    match answer {
        ScanAnswer::Fresh { last, .. } => last,
        ScanAnswer::Unchanged => panic!("expected a fresh scan"),
    }
}

#[test]
fn a_scan_with_nothing_changed_reads_nothing() {
    let finder = OneFile::new(true);
    let list = vec!["github".to_owned()];
    let last = fresh(scan_job(&finder, list.clone(), None));
    let answer = scan_job(&finder, list, Some(last.clone()));
    assert!(matches!(answer, ScanAnswer::Unchanged));
    assert_eq!(finder.runs.load(Ordering::SeqCst), 1);
    // The list changed (a removal): fresh.
    let other = fresh(scan_job(&finder, Vec::new(), Some(last)));
    assert_eq!(finder.runs.load(Ordering::SeqCst), 2);
    // The file changed: fresh.
    finder.touch();
    let _changed = fresh(scan_job(&finder, Vec::new(), Some(other)));
    assert_eq!(finder.runs.load(Ordering::SeqCst), 3);
}

#[test]
fn a_finder_that_knows_no_files_always_runs() {
    let finder = OneFile::new(false);
    let last = fresh(scan_job(&finder, Vec::new(), None));
    let _again = fresh(scan_job(&finder, Vec::new(), Some(last)));
    assert_eq!(finder.runs.load(Ordering::SeqCst), 2);
}
