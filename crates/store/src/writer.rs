//! The writer thread: the only connection that writes. It serves commits in groups, one
//! transaction and one sync per group, and keeps the streams' heads in memory.

use std::collections::HashMap;
use std::fs::File;

use async_channel::Receiver;
use clauseo_protocol::{AccountLimits, Seq, StreamId};
use futures::channel::oneshot;
use rusqlite::Connection;

use crate::open::Opened;
use crate::{Commit, Committed, Error, answer, codec};

mod apply;

/// A request to the writer thread.
pub(crate) enum WriteRequest {
    Commit {
        commit: Box<Commit>,
        reply: oneshot::Sender<Result<Committed, Error>>,
    },
    Heads {
        reply: oneshot::Sender<Result<Vec<(StreamId, Seq)>, Error>>,
    },
    PutAccountLimits {
        limits: Box<AccountLimits>,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    Close {
        reply: oneshot::Sender<Result<(), Error>>,
    },
    /// Blocks the writer until `release` resolves, so a test can queue requests that then
    /// form one group.
    #[cfg(feature = "test-support")]
    Hold {
        release: oneshot::Receiver<()>,
        reply: oneshot::Sender<Result<(), Error>>,
    },
    /// The value of a pragma on this connection, for tests.
    #[cfg(feature = "test-support")]
    Pragma {
        name: &'static str,
        reply: oneshot::Sender<Result<String, Error>>,
    },
}

/// A stream as the writer knows it: its number inside the store and its last `seq` (0 before
/// its first event).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StreamState {
    pub(crate) id: i64,
    pub(crate) last_seq: u64,
}

/// Where a commit's result goes.
type CommitTx = oneshot::Sender<Result<Committed, Error>>;

/// One commit waiting for its group's `COMMIT`.
struct Pending {
    reply: oneshot::Sender<Result<Committed, Error>>,
    result: Result<Committed, Error>,
}

pub(crate) struct Writer {
    conn: Connection,
    /// Held until the thread ends; dropping it releases the lock.
    lock: File,
    /// Every stream with a row, as of the last `COMMIT`.
    streams: HashMap<StreamId, StreamState>,
    /// The highest `stream_id` given so far.
    last_stream_id: i64,
    /// Reused between events, so encoding allocates only when an envelope outgrows it.
    buf: Vec<u8>,
}

impl Writer {
    /// Loads the streams map from the opened file.
    pub(crate) fn new(opened: Opened) -> Result<Self, Error> {
        let Opened { conn, lock, .. } = opened;
        let mut streams = HashMap::new();
        let mut last_stream_id = 0;
        {
            let mut statement =
                conn.prepare_cached("SELECT stream_id, stream, last_seq FROM streams")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let stream: StreamId = codec::parse(row.get_ref(1)?.as_str()?, "a stream id")?;
                let last_seq: i64 = row.get(2)?;
                let last_seq = u64::try_from(last_seq).map_err(|error| {
                    Error::corrupt(format!("{stream} has last_seq {last_seq}: {error}"))
                })?;
                last_stream_id = last_stream_id.max(id);
                streams.insert(stream, StreamState { id, last_seq });
            }
        }
        Ok(Self {
            conn,
            lock,
            streams,
            last_stream_id,
            buf: Vec::with_capacity(1024),
        })
    }

    /// Serves requests until `Close`, or until the queue is closed (by the last `Store` clone's
    /// drop) and empty; then closes as `Close` does.
    pub(crate) fn run(mut self, requests: &Receiver<WriteRequest>) {
        let mut next: Option<WriteRequest> = None;
        loop {
            let request = match next.take() {
                Some(request) => request,
                None => match requests.recv_blocking() {
                    Ok(request) => request,
                    Err(_) => break,
                },
            };
            match request {
                WriteRequest::Commit { commit, reply } => {
                    // Take every commit already waiting, without waiting for more; any other
                    // request ends the group and is served after it.
                    let mut group = vec![(commit, reply)];
                    while let Ok(request) = requests.try_recv() {
                        let WriteRequest::Commit { commit, reply } = request else {
                            next = Some(request);
                            break;
                        };
                        group.push((commit, reply));
                    }
                    self.commit_group(group);
                }
                WriteRequest::Heads { reply } => answer(reply, Ok(self.heads())),
                WriteRequest::PutAccountLimits { limits, reply } => {
                    answer(reply, apply::put_account_limits(&self.conn, &limits));
                }
                #[cfg(feature = "test-support")]
                WriteRequest::Hold { release, reply } => {
                    answer(reply, Ok(()));
                    // Sent or dropped, either ends the hold.
                    futures::executor::block_on(release).unwrap_or_default();
                }
                #[cfg(feature = "test-support")]
                WriteRequest::Pragma { name, reply } => {
                    answer(reply, crate::pragma(&self.conn, name));
                }
                WriteRequest::Close { reply } => {
                    requests.close();
                    answer(reply, self.close());
                    return;
                }
            }
        }
        // No caller is left to hear how the close went.
        drop(self.close());
    }

    /// Every stream with at least one event and its last `seq`, in stream order.
    fn heads(&self) -> Vec<(StreamId, Seq)> {
        let mut heads: Vec<(StreamId, Seq)> = self
            .streams
            .iter()
            .filter_map(|(stream, state)| Seq::new(state.last_seq).ok().map(|seq| (*stream, seq)))
            .collect();
        heads.sort_unstable();
        heads
    }

    /// Checkpoints the WAL into the file and truncates it, then closes the connection and
    /// releases the lock, in that order.
    fn close(self) -> Result<(), Error> {
        let checkpoint = self
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .map_err(Error::from);
        let closed = self.conn.close().map_err(|(_, error)| Error::from(error));
        drop(self.lock);
        checkpoint.and(closed)
    }

    /// Runs `group` in one transaction: a lone commit directly, several each in its own
    /// savepoint. Each caller's reply goes out only after the shared `COMMIT` returns.
    fn commit_group(&mut self, group: Vec<(Box<Commit>, CommitTx)>) {
        let mut state = Group {
            savepoints: group.len() > 1,
            done: Vec::with_capacity(group.len()),
            staged: HashMap::new(),
            staged_last_id: self.last_stream_id,
            open: false,
        };
        for (commit, reply) in group {
            if !state.open {
                if let Err(error) = self.exec("BEGIN IMMEDIATE") {
                    state.done.push(Pending {
                        reply,
                        result: Err(error),
                    });
                    continue;
                }
                state.open = true;
            }
            if state.savepoints
                && let Err(error) = self.exec("SAVEPOINT c")
            {
                self.fail_commit(&mut state, reply, error);
                continue;
            }
            let mut changes =
                apply::Changes::new(&self.streams, &state.staged, state.staged_last_id);
            match apply::apply(&self.conn, &mut self.buf, &commit, &mut changes) {
                Ok(committed) => {
                    if state.savepoints
                        && let Err(error) = self.exec("RELEASE c")
                    {
                        self.fail_commit(&mut state, reply, error);
                        continue;
                    }
                    let (streams, last_id) = changes.into_parts();
                    state.staged.extend(streams);
                    state.staged_last_id = last_id;
                    state.done.push(Pending {
                        reply,
                        result: Ok(committed),
                    });
                }
                Err(error) => self.fail_commit(&mut state, reply, error),
            }
        }
        if state.open {
            match self.exec("COMMIT") {
                Ok(()) => {
                    self.streams.extend(state.staged);
                    self.last_stream_id = state.staged_last_id;
                }
                Err(error) => {
                    self.rollback();
                    state.lose_released(&error);
                }
            }
        }
        for pending in state.done {
            answer(pending.reply, pending.result);
        }
    }

    /// Undoes the commit that just failed with `error`: a lone commit rolls the transaction
    /// back, a commit in a group rolls back to its savepoint. If SQLite already rolled the whole
    /// transaction back (it may on `SQLITE_FULL`, an I/O error or `SQLITE_BUSY`), or the
    /// savepoint cannot be undone, the commits released before it are lost too and get the same
    /// error, and the rest of the group starts a new transaction.
    fn fail_commit(
        &mut self,
        state: &mut Group,
        reply: oneshot::Sender<Result<Committed, Error>>,
        error: Error,
    ) {
        if self.conn.is_autocommit() {
            state.lose_released(&error);
            state.open = false;
        } else if !state.savepoints {
            self.rollback();
            state.open = false;
        } else if self
            .exec("ROLLBACK TO c")
            .and_then(|()| self.exec("RELEASE c"))
            .is_err()
        {
            self.rollback();
            state.lose_released(&error);
            state.open = false;
        }
        if !state.open {
            state.staged_last_id = self.last_stream_id;
        }
        state.done.push(Pending {
            reply,
            result: Err(error),
        });
    }

    /// Runs one statement that returns no rows, through the statement cache.
    fn exec(&self, sql: &str) -> Result<(), Error> {
        self.conn.prepare_cached(sql)?.execute([])?;
        Ok(())
    }

    /// Rolls the open transaction back, if SQLite has not already.
    fn rollback(&self) {
        if !self.conn.is_autocommit() {
            // A failed ROLLBACK leaves nothing to undo that the caller's error does not
            // already report; SQLite rolls back a transaction left open when the file is
            // next opened.
            drop(self.exec("ROLLBACK"));
        }
    }
}

/// The state of one group while it runs.
struct Group {
    /// Whether each commit runs in its own savepoint (more than one commit).
    savepoints: bool,
    /// Every commit handled so far, with its result.
    done: Vec<Pending>,
    /// The streams changed by commits whose savepoints were released, not yet committed.
    staged: HashMap<StreamId, StreamState>,
    /// The highest `stream_id` given, the staged streams included.
    staged_last_id: i64,
    /// Whether a transaction is open.
    open: bool,
}

impl Group {
    /// Fails every commit released so far with `error`: their transaction is gone.
    fn lose_released(&mut self, error: &Error) {
        for pending in &mut self.done {
            if pending.result.is_ok() {
                pending.result = Err(error.clone());
            }
        }
        self.staged.clear();
    }
}
