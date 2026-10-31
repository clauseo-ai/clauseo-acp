//! Shared helpers for the pipeline, sent view and snapshot tests: a temp folder per test, a
//! real store in it, fixed ids and times, and a post that collects commit results.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use clauseo_protocol::{DomainEvent, Notice, NoticeLevel, StreamId, ThreadId, Timestamp};
use clauseo_store::{Committed, Store, StoreOptions};
use futures::StreamExt;
use futures::channel::mpsc;
use futures::executor::LocalPool;
use std::rc::Rc;

use super::write;
use crate::ids::{Ids, SequentialIds};
use crate::pipeline::{Post, Ticket};

/// A folder under `std::env::temp_dir()` that one test owns; removed on drop.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// A fresh, empty folder named for `test` and this process.
    pub(crate) fn new(test: &str) -> Self {
        let path = std::env::temp_dir().join(format!("clauseo-host-{test}-{}", std::process::id()));
        write::remove_dir_all(&path);
        write::create_dir_all(&path);
        Self(path)
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        write::remove_dir_all(&self.0);
    }
}

/// Opens a store in `dir` on `pool`.
pub(crate) fn open_store(pool: &mut LocalPool, dir: &TempDir) -> Store {
    pool.run_until(Store::open(StoreOptions::new(dir.path().to_owned())))
        .unwrap()
}

/// A fixed v7 id numbered `n`.
pub(crate) fn id<T: FromStr>(n: u64) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01900000-0000-7000-8000-{n:012x}").parse().unwrap()
}

/// The time `ms` milliseconds after a fixed start.
pub(crate) fn ts(ms: i64) -> Timestamp {
    Timestamp::new(1_760_000_000_000 + ms).unwrap()
}

/// The test thread's stream.
pub(crate) fn thread_stream() -> StreamId {
    StreamId::Thread(id::<ThreadId>(1))
}

/// Deterministic ids.
pub(crate) fn ids() -> Ids {
    Ids::new(Box::new(SequentialIds::new()))
}

/// A notice event numbered `n`.
pub(crate) fn notice(n: usize) -> DomainEvent {
    DomainEvent::Notice(Notice {
        level: NoticeLevel::Info,
        text: format!("notice {n}"),
    })
}

/// Commit results, as the feeder posts them.
pub(crate) struct Results(
    mpsc::UnboundedReceiver<(Ticket, Result<Committed, clauseo_store::Error>)>,
);

impl Results {
    /// A post and the results it collects.
    pub(crate) fn new() -> (Post, Self) {
        let (sender, receiver) = mpsc::unbounded();
        let post: Post = Rc::new(move |ticket, result| {
            sender.unbounded_send((ticket, result)).unwrap();
        });
        (post, Self(receiver))
    }

    /// Waits for the next posted result.
    pub(crate) fn next(
        &mut self,
        pool: &mut LocalPool,
    ) -> (Ticket, Result<Committed, clauseo_store::Error>) {
        pool.run_until(self.0.next()).unwrap()
    }

    /// The result already posted, if any, without waiting.
    pub(crate) fn ready(&mut self) -> Option<(Ticket, Result<Committed, clauseo_store::Error>)> {
        self.0.try_recv().ok()
    }
}
