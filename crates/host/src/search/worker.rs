//! The search thread's loop: it opens the index, rebuilds it from the log when it must, and
//! then serves control jobs (searches first), commits from the actor, a lag's catch-up and the
//! walks a rebuild or a catch-up queued, in that order.
//!
//! Every write lands in one transaction with its stream's watermark, the last seq the index
//! reflects; a stream is read from the store after its watermark whenever the thread lagged
//! (the actor's channel was full), at start-up, and for a commit that does not follow on.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use async_channel::{Receiver, TryRecvError};
use clauseo_protocol::{
    Envelope, MatterId, SearchAnswer, SearchRequest, Seq, StreamId, ThreadId, Timestamp,
};
use clauseo_search::SearchIndex;
use clauseo_store::{READ_PAGE_MAX, Store};
use futures::executor::block_on;
use futures::future::{Either, select};

use super::batch::{Failure, write_chunk};
use super::files::Files;
use super::fold::{ThreadMemo, Write, feed_matter};
use super::{Control, SearchReply, SearchSeams, Shared, answer};
use crate::clock::Clock;
use crate::error::Error;
use crate::matter_worker::matter_settings;

/// Threads whose fold the thread keeps; another is folded again from the log when needed.
const MEMO_CAP: usize = 64;

/// What the thread starts with.
pub(super) struct Parts {
    pub(super) data_dir: PathBuf,
    pub(super) store: Store,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) shared: Arc<Shared>,
    pub(super) commits: Receiver<Vec<Envelope>>,
    pub(super) control: Receiver<Control>,
    pub(super) seams: SearchSeams,
}

/// One stream to read after its watermark; `counts` marks a rebuild's conversation.
#[derive(Debug, Clone, Copy)]
struct Walk {
    stream: StreamId,
    counts: bool,
}

/// What woke the idle thread.
enum Wake {
    Control(Control),
    Commits(Vec<Envelope>),
    Released,
    Closed,
}

struct Worker {
    index: SearchIndex,
    store: Store,
    clock: Arc<dyn Clock>,
    shared: Arc<Shared>,
    commits: Receiver<Vec<Envelope>>,
    control: Receiver<Control>,
    seams: SearchSeams,
    files: Files,
    marks: BTreeMap<StreamId, Seq>,
    memos: BTreeMap<ThreadId, ThreadMemo>,
    /// The memos, least recently used first.
    memo_order: VecDeque<ThreadId>,
    forgotten: BTreeSet<MatterId>,
    thread_matter: BTreeMap<ThreadId, MatterId>,
    pending: VecDeque<Walk>,
    /// Control jobs other than searches that came during a walk.
    deferred: VecDeque<Control>,
    rebuilding: bool,
}

/// The thread's body.
pub(super) fn run(parts: Parts) {
    let Parts {
        data_dir,
        store,
        clock,
        shared,
        commits,
        control,
        seams,
    } = parts;
    let (index, state) = match SearchIndex::open(&data_dir) {
        Ok(opened) => opened,
        Err(error) => {
            tracing::error!(%error, "the search index did not open; search is off");
            unavailable(&commits, &control);
            return;
        }
    };
    shared.generations().set_interrupter(index.interrupter());
    let files = Files::new(store.clone(), matter_settings(&data_dir));
    let mut worker = Worker {
        index,
        store,
        clock,
        shared,
        commits,
        control,
        seams,
        files,
        marks: BTreeMap::new(),
        memos: BTreeMap::new(),
        memo_order: VecDeque::new(),
        forgotten: BTreeSet::new(),
        thread_matter: BTreeMap::new(),
        pending: VecDeque::new(),
        deferred: VecDeque::new(),
        rebuilding: false,
    };
    if let Err(failure) = worker.begin(state.needs_rebuild) {
        tracing::warn!(%failure, "the search index did not start its catch-up");
    }
    worker.serve();
}

/// Answers every search with an error until the host stops, when the index did not open.
fn unavailable(commits: &Receiver<Vec<Envelope>>, control: &Receiver<Control>) {
    while let Ok(job) = block_on(control.recv()) {
        if let Control::Query { reply, .. } = job {
            reply(Err(Error::Internal("the search index is off".to_owned())));
        }
        while commits.try_recv().is_ok() {}
    }
}

impl Worker {
    /// Reads the watermarks and the matters, then queues a rebuild or a catch-up.
    fn begin(&mut self, rebuild: bool) -> Result<(), Failure> {
        self.marks = self.index.watermarks()?.into_iter().collect();
        for row in block_on(self.store.matters())? {
            if row.forgotten.is_some() {
                self.forgotten.insert(row.id);
            }
            self.files.note(&row);
            for thread in block_on(self.store.threads(row.id))? {
                self.thread_matter.insert(thread.id, row.id);
            }
        }
        if rebuild {
            self.start_rebuild()
        } else {
            self.catch_up_all()
        }
    }

    /// Queues every stream of every matter on the list, conversations least recently active
    /// first, so the newest rows get the highest row ids (newest first pages by them).
    fn start_rebuild(&mut self) -> Result<(), Failure> {
        let tails: BTreeMap<StreamId, Timestamp> =
            block_on(self.store.tail_times())?.into_iter().collect();
        let mut threads: Vec<(Option<Timestamp>, ThreadId)> = self
            .thread_matter
            .iter()
            .filter(|(_, matter)| !self.forgotten.contains(matter))
            .map(|(&thread, _)| (tails.get(&StreamId::Thread(thread)).copied(), thread))
            .collect();
        threads.sort_unstable();
        self.index.start_rebuild(threads.len() as u64)?;
        self.rebuilding = true;
        let matters: Vec<MatterId> = self.files.matters().collect();
        for matter in matters {
            if !self.forgotten.contains(&matter) {
                self.pending.push_back(Walk {
                    stream: StreamId::Matter(matter),
                    counts: false,
                });
            }
        }
        for (_, thread) in threads {
            self.pending.push_back(Walk {
                stream: StreamId::Thread(thread),
                counts: true,
            });
        }
        Ok(())
    }

    /// Queues every stream whose head is past its watermark.
    fn catch_up_all(&mut self) -> Result<(), Failure> {
        for (stream, head) in block_on(self.store.heads())? {
            if self.marks.get(&stream).is_none_or(|&mark| mark < head) {
                self.pending.push_back(Walk {
                    stream,
                    counts: false,
                });
            }
        }
        Ok(())
    }

    fn serve(&mut self) {
        loop {
            if let Some(job) = self.deferred.pop_front() {
                self.control_job(job);
                continue;
            }
            match self.control.try_recv() {
                Ok(job) => {
                    self.control_job(job);
                    continue;
                }
                Err(TryRecvError::Closed) => return,
                Err(TryRecvError::Empty) => {}
            }
            match self.commits.try_recv() {
                Ok(batch) => {
                    self.commit(&batch);
                    continue;
                }
                Err(TryRecvError::Closed) => return,
                Err(TryRecvError::Empty) => {}
            }
            if self.shared.lagged.swap(false, Ordering::SeqCst) {
                if let Err(failure) = self.catch_up_all() {
                    tracing::warn!(%failure, "the search index did not catch up");
                }
                continue;
            }
            let held = self.rebuild_held();
            if !held && let Some(walk) = self.pending.pop_front() {
                self.walk(walk);
                continue;
            }
            if !held && self.rebuilding {
                self.rebuilding = false;
                if let Err(error) = self.index.finish_rebuild() {
                    tracing::warn!(%error, "the search index's rebuild did not finish");
                }
                continue;
            }
            match self.wait(held) {
                Wake::Control(job) => self.control_job(job),
                Wake::Commits(batch) => self.commit(&batch),
                Wake::Released => {}
                Wake::Closed => return,
            }
        }
    }

    /// Whether the rebuild hold stops the next walk (test seam).
    fn rebuild_held(&self) -> bool {
        let next_counts = self.pending.front().is_some_and(|walk| walk.counts);
        match &self.seams.rebuild_hold {
            Some(hold) if self.rebuilding && next_counts && !hold.is_released() => {
                hold.note_reached();
                true
            }
            Some(_) | None => false,
        }
    }

    /// Waits for a control job, a commit or (when `held`) the hold's release.
    fn wait(&self, held: bool) -> Wake {
        let hold = self.seams.rebuild_hold.clone().filter(|_| held);
        block_on(async {
            let control = pin!(self.control.recv());
            let commits = pin!(self.commits.recv());
            let released = pin!(async move {
                match hold {
                    Some(hold) => hold.wait().await,
                    None => futures::future::pending::<()>().await,
                }
            });
            match select(control, select(commits, released)).await {
                Either::Left((job, _)) => job.map_or(Wake::Closed, Wake::Control),
                Either::Right((Either::Left((batch, _)), _)) => {
                    batch.map_or(Wake::Closed, Wake::Commits)
                }
                Either::Right((Either::Right(((), _)), _)) => Wake::Released,
            }
        })
    }

    fn control_job(&mut self, job: Control) {
        match job {
            Control::Query {
                window,
                request,
                reply,
            } => self.answer(&window, &request, reply),
            Control::Forget(matter) => {
                self.forgotten.insert(matter);
                if let Err(failure) = self.drop_matter(matter) {
                    tracing::warn!(%matter, %failure, "a forgotten matter's rows were not deleted");
                }
            }
            Control::Reindex(matter) => {
                if let Err(failure) = self.reindex(matter) {
                    tracing::warn!(%matter, %failure, "a matter was not indexed again");
                }
            }
        }
    }

    /// Runs one search, unless a newer one from its window came first.
    fn answer(&mut self, window: &str, request: &SearchRequest, reply: SearchReply) {
        let generation = request.generation.get();
        if !self.shared.generations().start(window, generation) {
            reply(Ok(SearchAnswer::Cancelled));
            return;
        }
        if let Some(hold) = &self.seams.query_hold
            && !hold.is_released()
        {
            hold.note_reached();
            block_on(hold.wait());
        }
        let query = answer::query(request, self.clock.now().wall);
        let result = self.index.query(&query);
        let superseded = {
            let mut generations = self.shared.generations();
            generations.finish();
            generations.superseded(window, generation)
        };
        let answered = match result {
            // A late answer to an older search is never delivered.
            Ok(_) | Err(clauseo_search::Error::Interrupted) if superseded => {
                Ok(SearchAnswer::Cancelled)
            }
            Err(clauseo_search::Error::Interrupted) => Ok(SearchAnswer::Cancelled),
            Ok(page) => Ok(SearchAnswer::Page(answer::page(page))),
            Err(error) => Err(Error::Internal(format!("search: {error}"))),
        };
        reply(answered);
    }

    /// Answers the searches queued now, keeping the other control jobs for later: a long walk
    /// does not hold searches up.
    fn answer_queued(&mut self) {
        while let Ok(job) = self.control.try_recv() {
            if let Control::Query {
                window,
                request,
                reply,
            } = job
            {
                self.answer(&window, &request, reply);
            } else {
                self.deferred.push_back(job);
            }
        }
    }

    /// Folds a commit's envelopes, stream by stream; a stream whose events do not follow on
    /// from its watermark is read from the store instead.
    fn commit(&mut self, batch: &[Envelope]) {
        let mut streams: Vec<StreamId> = Vec::new();
        for envelope in batch {
            if !streams.contains(&envelope.stream) {
                streams.push(envelope.stream);
            }
        }
        for stream in streams {
            let mark = self.marks.get(&stream).copied();
            let fresh: Vec<&Envelope> = batch
                .iter()
                .filter(|envelope| {
                    envelope.stream == stream && mark.is_none_or(|m| envelope.seq > m)
                })
                .collect();
            let Some(first) = fresh.first() else {
                continue;
            };
            if self.is_forgotten(stream) {
                continue;
            }
            if first.seq.get() != mark.map_or(1, |mark| mark.get().saturating_add(1)) {
                self.walk(Walk {
                    stream,
                    counts: false,
                });
                continue;
            }
            if let Err(failure) = self.fold_and_apply(stream, &fresh) {
                tracing::warn!(%stream, %failure, "a commit did not reach the search index");
                self.forget_memo(stream);
            }
        }
    }

    /// Reads `walk`'s stream after its watermark to its end.
    fn walk(&mut self, walk: Walk) {
        if !self.is_forgotten(walk.stream)
            && let Err(failure) = self.catch_up(walk.stream)
        {
            tracing::warn!(stream = %walk.stream, %failure, "a stream did not reach the search index");
            self.forget_memo(walk.stream);
        }
        if walk.counts {
            let counted = self.index.batch(|batch| {
                batch.conversation_done();
                Ok(())
            });
            if let Err(error) = counted {
                tracing::warn!(%error, "the rebuild's progress was not counted");
            }
        }
    }

    fn catch_up(&mut self, stream: StreamId) -> Result<(), Failure> {
        loop {
            let after = self.marks.get(&stream).copied();
            let page = block_on(self.store.read_after(stream, after, READ_PAGE_MAX))?;
            if page.is_empty() {
                return Ok(());
            }
            let page: Vec<&Envelope> = page.iter().collect();
            self.fold_and_apply(stream, &page)?;
            self.answer_queued();
        }
    }

    /// Folds envelopes that follow on from the stream's watermark and writes what they make.
    fn fold_and_apply(&mut self, stream: StreamId, envelopes: &[&Envelope]) -> Result<(), Failure> {
        let Some(last) = envelopes.last().map(|envelope| envelope.seq) else {
            return Ok(());
        };
        let mut writes = Vec::new();
        match stream {
            StreamId::Matter(matter) => {
                for envelope in envelopes {
                    feed_matter(matter, envelope, &mut writes);
                }
            }
            StreamId::Thread(thread) => {
                let memo = self.memo(thread)?;
                for envelope in envelopes {
                    memo.feed(envelope, &mut writes);
                }
                if let Some(matter) = memo.matter() {
                    self.thread_matter.insert(thread, matter);
                }
            }
        }
        self.apply(stream, writes, last)
    }

    /// Writes `writes` and moves the watermark to `last`: one transaction, split only around an
    /// undone step's deletes (each its own, with a WAL checkpoint). A failure leaves the
    /// watermark where it was.
    fn apply(&mut self, stream: StreamId, writes: Vec<Write>, last: Seq) -> Result<(), Failure> {
        let files = writes.iter().rev().find_map(|write| match write {
            Write::Files {
                matter,
                at,
                checkpoint,
            } => Some((*matter, *at, *checkpoint)),
            Write::Entry { .. }
            | Write::ThreadTitle { .. }
            | Write::MatterTitle { .. }
            | Write::Archived { .. }
            | Write::Undone { .. } => None,
        });
        let listing = files.and_then(|(matter, at, checkpoint)| {
            self.files
                .list(matter, checkpoint)
                .map(|paths| (matter, at, paths))
        });
        let mut chunk: Vec<Write> = Vec::new();
        for write in writes {
            match write {
                Write::Undone { thread, entries } => {
                    write_chunk(&mut self.index, &chunk, None, None)?;
                    chunk.clear();
                    self.index.delete_entries(thread, entries)?;
                }
                Write::Files { .. } => {}
                Write::Entry { .. }
                | Write::ThreadTitle { .. }
                | Write::MatterTitle { .. }
                | Write::Archived { .. } => chunk.push(write),
            }
        }
        write_chunk(
            &mut self.index,
            &chunk,
            listing.as_ref(),
            Some((stream, last)),
        )?;
        self.marks.insert(stream, last);
        Ok(())
    }

    /// The thread's fold up to its watermark, folded again from the log when not kept.
    fn memo(&mut self, thread: ThreadId) -> Result<&mut ThreadMemo, Failure> {
        if self.memos.contains_key(&thread) {
            self.memo_order.retain(|kept| *kept != thread);
        } else {
            let memo = self.load_memo(thread)?;
            self.memos.insert(thread, memo);
            if self.memo_order.len() >= MEMO_CAP
                && let Some(oldest) = self.memo_order.pop_front()
            {
                self.memos.remove(&oldest);
            }
        }
        self.memo_order.push_back(thread);
        self.memos
            .get_mut(&thread)
            .ok_or_else(|| Failure::Index(clauseo_search::Error::Corrupt("memo".to_owned())))
    }

    fn load_memo(&self, thread: ThreadId) -> Result<ThreadMemo, Failure> {
        let stream = StreamId::Thread(thread);
        let mut memo = ThreadMemo::new(thread);
        let Some(upto) = self.marks.get(&stream).copied() else {
            return Ok(memo);
        };
        let mut sink = Vec::new();
        let mut after = None;
        while after < Some(upto) {
            let page = block_on(self.store.read_after(stream, after, READ_PAGE_MAX))?;
            if page.is_empty() {
                break;
            }
            for envelope in page.iter().filter(|envelope| envelope.seq <= upto) {
                memo.feed(envelope, &mut sink);
                sink.clear();
            }
            after = page.last().map(|envelope| envelope.seq);
        }
        Ok(memo)
    }

    fn forget_memo(&mut self, stream: StreamId) {
        if let StreamId::Thread(thread) = stream {
            self.memos.remove(&thread);
            self.memo_order.retain(|kept| *kept != thread);
        }
    }

    fn is_forgotten(&self, stream: StreamId) -> bool {
        let matter = match stream {
            StreamId::Matter(matter) => Some(matter),
            StreamId::Thread(thread) => self.thread_matter.get(&thread).copied(),
        };
        matter.is_some_and(|matter| self.forgotten.contains(&matter))
    }

    /// Deletes the matter's rows and its streams' watermarks.
    fn drop_matter(&mut self, matter: MatterId) -> Result<(), Failure> {
        self.index.delete_matter(matter)?;
        let mut streams = vec![StreamId::Matter(matter)];
        streams.extend(
            self.thread_matter
                .iter()
                .filter(|(_, owner)| **owner == matter)
                .map(|(&thread, _)| StreamId::Thread(thread)),
        );
        self.index.batch(|batch| {
            for &stream in &streams {
                batch.set_watermark(stream, None)?;
            }
            Ok(())
        })?;
        for stream in streams {
            self.marks.remove(&stream);
            self.forget_memo(stream);
        }
        self.files.close(matter);
        Ok(())
    }

    /// Indexes a matter that came back from its streams' whole logs, before other walks.
    fn reindex(&mut self, matter: MatterId) -> Result<(), Failure> {
        self.forgotten.remove(&matter);
        for thread in block_on(self.store.threads(matter))? {
            self.thread_matter.insert(thread.id, matter);
        }
        self.files.refresh();
        self.drop_matter(matter)?;
        let mut walks = vec![StreamId::Matter(matter)];
        walks.extend(
            self.thread_matter
                .iter()
                .filter(|(_, owner)| **owner == matter)
                .map(|(&thread, _)| StreamId::Thread(thread)),
        );
        for stream in walks.into_iter().rev() {
            self.pending.push_front(Walk {
                stream,
                counts: false,
            });
        }
        Ok(())
    }
}
