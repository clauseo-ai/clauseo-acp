//! Helpers shared by the test modules.

use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterId, Seq, ThreadId, Timestamp};
use clauseo_search::{EntryKind, Hit, Mode, Page, Query, SearchIndex};

use crate::write;

/// A fresh folder for one test, `CARGO_TARGET_TMPDIR/clauseo-search-<name>-<pid>`, removed on
/// drop.
pub(crate) struct TestDir(PathBuf);

impl TestDir {
    pub(crate) fn new(name: &str) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("clauseo-search-{name}-{}", std::process::id()));
        write::remove_dir_all(&path);
        write::create_dir_all(&path);
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        write::remove_dir_all(&self.0);
    }
}

/// A built (complete) empty index in `dir`.
pub(crate) fn index(dir: &TestDir) -> SearchIndex {
    let (mut index, _) = SearchIndex::open(dir.path()).unwrap();
    index.finish_rebuild().unwrap();
    index
}

/// A fixed v7 matter id whose low bits are `n`.
pub(crate) fn matter(n: u64) -> MatterId {
    format!("0190a000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// A fixed v7 thread id whose low bits are `n`.
pub(crate) fn thread(n: u64) -> ThreadId {
    format!("0190b000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// `n` minutes after a fixed time in 2024.
pub(crate) fn at(n: i64) -> Timestamp {
    Timestamp::new(1_730_000_000_000 + n * 60_000).unwrap()
}

pub(crate) fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

/// Puts an answer in matter `m`, thread `t`, entry `e` at minute `e`.
pub(crate) fn answer(index: &mut SearchIndex, m: u64, t: u64, e: u64, text: &str) {
    let minute = i64::try_from(e).unwrap();
    index
        .put_entry(
            matter(m),
            thread(t),
            seq(e),
            EntryKind::Answer,
            at(minute),
            text,
        )
        .unwrap();
}

/// A first-page query over everything, not archived, 50 hits.
pub(crate) fn query(text: &str, mode: Mode) -> Query {
    Query {
        text: text.to_owned(),
        matter: None,
        kinds: None,
        include_archived: false,
        mode,
        cursor: None,
        limit: 50,
        now: at(100_000),
    }
}

/// The page for `text` in `mode`.
pub(crate) fn find(index: &SearchIndex, text: &str, mode: Mode) -> Page {
    index.query(&query(text, mode)).unwrap()
}

/// How many hits the first page for `text` in `mode` has.
pub(crate) fn count(index: &SearchIndex, text: &str, mode: Mode) -> usize {
    find(index, text, mode).hits.len()
}

/// The snippet as one string.
pub(crate) fn snippet(hit: &Hit) -> String {
    hit.snippet.iter().map(|part| part.text.as_str()).collect()
}

/// The parts marked as hits.
pub(crate) fn marked(hit: &Hit) -> Vec<&str> {
    hit.snippet
        .iter()
        .filter(|part| part.hit)
        .map(|part| part.text.as_str())
        .collect()
}

/// The (thread, entry) of each hit, as the numbers the helpers took.
pub(crate) fn keys(page: &Page) -> Vec<(Option<ThreadId>, Option<u64>)> {
    page.hits
        .iter()
        .map(|hit| (hit.thread, hit.entry.map(Seq::get)))
        .collect()
}
