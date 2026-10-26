//! `Store`: the handle callers hold. Each method sends a request to the writer or the reader
//! thread and awaits its reply on a oneshot; neither thread ever runs on the caller's executor.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::thread::JoinHandle;

use async_channel::{Receiver, Sender};
use clauseo_protocol::{
    AccountLimits, BlobHash, CheckpointId, CommandId, Envelope, MatterId, Seq, StreamId,
};
use futures::channel::oneshot;
use rusqlite::Connection;

use crate::read::{self, StreamIds};
use crate::writer::{WriteRequest, Writer};
use crate::{
    CheckpointRow, Commit, Committed, Error, MatterRow, Receipt, Reopen, StoreOptions, ThreadRow,
    answer, blob, open,
};

/// Requests each thread's queue holds before `send` waits.
const QUEUE: usize = 256;

/// The store: one SQLite file served by a writer thread and a reader thread. Cheap to clone;
/// every clone talks to the same threads. Calls after [`Store::close`] return
/// [`Error::Closed`].
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    writer: Sender<WriteRequest>,
    reader: Sender<ReadRequest>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

/// A request to the reader thread.
enum ReadRequest {
    ReadAfter {
        stream: StreamId,
        after: Option<Seq>,
        limit: u32,
        reply: oneshot::Sender<Result<Vec<Envelope>, Error>>,
    },
    Reopen {
        stream: StreamId,
        reducer_schema: u32,
        reply: oneshot::Sender<Result<Reopen, Error>>,
    },
    Receipt {
        command: CommandId,
        reply: oneshot::Sender<Result<Option<Receipt>, Error>>,
    },
    ReadBlob {
        hash: BlobHash,
        offset: u64,
        len: u32,
        reply: oneshot::Sender<Result<Vec<u8>, Error>>,
    },
    Matters {
        reply: oneshot::Sender<Result<Vec<MatterRow>, Error>>,
    },
    Threads {
        matter: MatterId,
        reply: oneshot::Sender<Result<Vec<ThreadRow>, Error>>,
    },
    Checkpoint {
        id: CheckpointId,
        reply: oneshot::Sender<Result<Option<CheckpointRow>, Error>>,
    },
    AccountLimits {
        reply: oneshot::Sender<Result<Vec<AccountLimits>, Error>>,
    },
    Close {
        reply: oneshot::Sender<Result<(), Error>>,
    },
    #[cfg(feature = "test-support")]
    Pragma {
        name: &'static str,
        reply: oneshot::Sender<Result<String, Error>>,
    },
}

/// The reply to [`Store::send_commit`]: resolves once the commit is durable (its group's
/// `COMMIT` returned), or with its error.
#[must_use = "the commit is durable only when this resolves Ok"]
pub struct CommitReply(oneshot::Receiver<Result<Committed, Error>>);

impl Future for CommitReply {
    type Output = Result<Committed, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0)
            .poll(cx)
            .map(|reply| reply.unwrap_or(Err(Error::Closed)))
    }
}

impl Store {
    /// Opens (or creates) `state.sqlite` in `options.dir`: takes the exclusive lock, creates the
    /// file with mode `0o600` on Unix, sets the pragmas, runs the migrations, and starts the
    /// `clauseo-store-writer` and `clauseo-store-reader` threads.
    pub async fn open(options: StoreOptions) -> Result<Self, Error> {
        let (writer_tx, writer_rx) = async_channel::bounded(QUEUE);
        let (ready_tx, ready_rx) = oneshot::channel();
        let writer = std::thread::Builder::new()
            .name("clauseo-store-writer".to_owned())
            .spawn(move || run_writer(&options, ready_tx, &writer_rx))?;
        let path = match ready_rx.await {
            Ok(Ok(path)) => path,
            Ok(Err(error)) => {
                join(writer);
                return Err(error);
            }
            Err(_) => {
                join(writer);
                return Err(Error::Closed);
            }
        };
        let (reader_tx, reader_rx) = async_channel::bounded(QUEUE);
        let (ready_tx, ready_rx) = oneshot::channel();
        let reader = std::thread::Builder::new()
            .name("clauseo-store-reader".to_owned())
            .spawn(move || run_reader(&path, ready_tx, &reader_rx));
        let store = Self {
            inner: Arc::new(Inner {
                writer: writer_tx,
                reader: reader_tx,
                threads: Mutex::new(vec![writer]),
            }),
        };
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                drop(store.close().await);
                return Err(error.into());
            }
        };
        store.join_handles().push(reader);
        match ready_rx.await.unwrap_or(Err(Error::Closed)) {
            Ok(()) => Ok(store),
            Err(error) => {
                drop(store.close().await);
                Err(error)
            }
        }
    }

    /// The last `seq` of every stream that has events, in stream order.
    pub async fn heads(&self) -> Result<Vec<(StreamId, Seq)>, Error> {
        self.write(|reply| WriteRequest::Heads { reply }).await
    }

    /// Writes `commit` and returns once it is durable: [`Store::send_commit`], then its reply.
    pub async fn commit(&self, commit: Commit) -> Result<Committed, Error> {
        self.send_commit(commit).await?.await
    }

    /// Queues `commit` and returns as soon as it is in the writer's queue; the [`CommitReply`]
    /// resolves when it is durable. Commits from one caller that awaits each `send_commit` in
    /// turn reach the writer in that order, so a stream's commits can be pipelined and share a
    /// group.
    pub async fn send_commit(&self, commit: Commit) -> Result<CommitReply, Error> {
        let (reply, receiver) = oneshot::channel();
        self.inner
            .writer
            .send(WriteRequest::Commit {
                commit: Box::new(commit),
                reply,
            })
            .await
            .map_err(closed)?;
        Ok(CommitReply(receiver))
    }

    /// Up to `limit` envelopes of `stream` after `after` (from the first when `None`), in `seq`
    /// order. `limit` is capped at [`READ_PAGE_MAX`](crate::READ_PAGE_MAX).
    pub async fn read_after(
        &self,
        stream: StreamId,
        after: Option<Seq>,
        limit: u32,
    ) -> Result<Vec<Envelope>, Error> {
        self.read(|reply| ReadRequest::ReadAfter {
            stream,
            after,
            limit,
            reply,
        })
        .await
    }

    /// The newest snapshot of `stream` written with `reducer_schema`, decompressed, and every
    /// event after it. A snapshot of another schema is ignored.
    pub async fn reopen(&self, stream: StreamId, reducer_schema: u32) -> Result<Reopen, Error> {
        self.read(|reply| ReadRequest::Reopen {
            stream,
            reducer_schema,
            reply,
        })
        .await
    }

    /// The receipt of `command`, or `None` (also when the stored one no longer decodes).
    pub async fn receipt(&self, command: CommandId) -> Result<Option<Receipt>, Error> {
        self.read(|reply| ReadRequest::Receipt { command, reply })
            .await
    }

    /// Up to `len` bytes of the blob `hash` from `offset`, cut at its end and at
    /// [`MAX_BLOB_READ_BYTES`](clauseo_protocol::MAX_BLOB_READ_BYTES).
    pub async fn read_blob(&self, hash: BlobHash, offset: u64, len: u32) -> Result<Vec<u8>, Error> {
        self.read(|reply| ReadRequest::ReadBlob {
            hash,
            offset,
            len,
            reply,
        })
        .await
    }

    /// Every matter in the registry, oldest first.
    pub async fn matters(&self) -> Result<Vec<MatterRow>, Error> {
        self.read(|reply| ReadRequest::Matters { reply }).await
    }

    /// The threads of `matter`, most recently updated first.
    pub async fn threads(&self, matter: MatterId) -> Result<Vec<ThreadRow>, Error> {
        self.read(|reply| ReadRequest::Threads { matter, reply })
            .await
    }

    /// The checkpoint `id`.
    pub async fn checkpoint(&self, id: CheckpointId) -> Result<Option<CheckpointRow>, Error> {
        self.read(|reply| ReadRequest::Checkpoint { id, reply })
            .await
    }

    /// Keeps `limits` as the last value for its agent and account.
    pub async fn put_account_limits(&self, limits: AccountLimits) -> Result<(), Error> {
        self.write(|reply| WriteRequest::PutAccountLimits {
            limits: Box::new(limits),
            reply,
        })
        .await
    }

    /// The last limits of every account (values that no longer decode are left out).
    pub async fn account_limits(&self) -> Result<Vec<AccountLimits>, Error> {
        self.read(|reply| ReadRequest::AccountLimits { reply })
            .await
    }

    /// Stops the reader, checkpoints and truncates the WAL, closes the file, releases the lock
    /// and joins both threads. Every clone's later calls return [`Error::Closed`].
    pub async fn close(self) -> Result<(), Error> {
        let read = self.read(|reply| ReadRequest::Close { reply }).await;
        let wrote = self.write(|reply| WriteRequest::Close { reply }).await;
        self.inner.reader.close();
        self.inner.writer.close();
        let threads = std::mem::take(&mut *self.join_handles());
        for thread in threads {
            join(thread);
        }
        // A second close finds the threads gone; that is not a failure.
        match (read, wrote) {
            (Err(Error::Closed), Err(Error::Closed)) => Ok(()),
            (read, wrote) => read.and(wrote),
        }
    }

    /// Holds the writer thread until the returned sender is dropped or sent to: requests sent
    /// meanwhile wait in its queue, so the commits among them form one group. Returns once the
    /// writer is held. Test support only.
    #[cfg(feature = "test-support")]
    pub async fn hold_writer(&self) -> Result<oneshot::Sender<()>, Error> {
        let (release, held) = oneshot::channel();
        self.write(|reply| WriteRequest::Hold {
            release: held,
            reply,
        })
        .await?;
        Ok(release)
    }

    /// The value of `PRAGMA <name>` on the writer's connection, as text. Test support only.
    #[cfg(feature = "test-support")]
    pub async fn writer_pragma(&self, name: &'static str) -> Result<String, Error> {
        self.write(|reply| WriteRequest::Pragma { name, reply })
            .await
    }

    /// The value of `PRAGMA <name>` on the reader's connection, as text. Test support only.
    #[cfg(feature = "test-support")]
    pub async fn reader_pragma(&self, name: &'static str) -> Result<String, Error> {
        self.read(|reply| ReadRequest::Pragma { name, reply }).await
    }

    fn join_handles(&self) -> std::sync::MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.inner
            .threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    async fn write<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> WriteRequest,
    ) -> Result<T, Error> {
        let (reply, receiver) = oneshot::channel();
        self.inner
            .writer
            .send(request(reply))
            .await
            .map_err(closed)?;
        receiver.await.unwrap_or(Err(Error::Closed))
    }

    async fn read<T>(
        &self,
        request: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> ReadRequest,
    ) -> Result<T, Error> {
        let (reply, receiver) = oneshot::channel();
        self.inner
            .reader
            .send(request(reply))
            .await
            .map_err(closed)?;
        receiver.await.unwrap_or(Err(Error::Closed))
    }
}

/// A send to a thread whose queue is closed: the store is closed.
fn closed<T>(_unsent: async_channel::SendError<T>) -> Error {
    Error::Closed
}

/// Joins a store thread. Its loop returns no error and catches none: a panic there is a bug
/// that already failed its caller's call with `Closed`.
fn join(thread: JoinHandle<()>) {
    drop(thread.join());
}

/// The writer thread: opens the file, says so, then serves requests.
fn run_writer(
    options: &StoreOptions,
    ready: oneshot::Sender<Result<std::path::PathBuf, Error>>,
    requests: &Receiver<WriteRequest>,
) {
    let opened = open::open_writer(options).and_then(|opened| {
        let path = opened.path.clone();
        Writer::new(opened).map(|writer| (writer, path))
    });
    match opened {
        Ok((writer, path)) => {
            answer(ready, Ok(path));
            writer.run(requests);
        }
        Err(error) => answer(ready, Err(error)),
    }
}

/// The reader thread: opens its connection, says so, then serves requests.
fn run_reader(
    path: &std::path::Path,
    ready: oneshot::Sender<Result<(), Error>>,
    requests: &Receiver<ReadRequest>,
) {
    let conn = match open::open_reader(path) {
        Ok(conn) => {
            answer(ready, Ok(()));
            conn
        }
        Err(error) => {
            answer(ready, Err(error));
            return;
        }
    };
    let mut ids = StreamIds::default();
    while let Ok(request) = requests.recv_blocking() {
        if let Some(reply) = serve(&conn, &mut ids, request) {
            requests.close();
            answer(reply, conn.close().map_err(|(_, error)| Error::from(error)));
            return;
        }
    }
}

/// Serves one read; returns the reply of a `Close`, which the loop answers after closing.
fn serve(
    conn: &Connection,
    ids: &mut StreamIds,
    request: ReadRequest,
) -> Option<oneshot::Sender<Result<(), Error>>> {
    match request {
        ReadRequest::ReadAfter {
            stream,
            after,
            limit,
            reply,
        } => answer(reply, read::read_after(conn, ids, stream, after, limit)),
        ReadRequest::Reopen {
            stream,
            reducer_schema,
            reply,
        } => answer(reply, read::reopen(conn, ids, stream, reducer_schema)),
        ReadRequest::Receipt { command, reply } => answer(reply, read::receipt(conn, command)),
        ReadRequest::ReadBlob {
            hash,
            offset,
            len,
            reply,
        } => answer(reply, blob::read(conn, hash, offset, len)),
        ReadRequest::Matters { reply } => answer(reply, read::matters(conn)),
        ReadRequest::Threads { matter, reply } => answer(reply, read::threads(conn, matter)),
        ReadRequest::Checkpoint { id, reply } => answer(reply, read::checkpoint(conn, id)),
        ReadRequest::AccountLimits { reply } => answer(reply, read::account_limits(conn)),
        #[cfg(feature = "test-support")]
        ReadRequest::Pragma { name, reply } => answer(reply, crate::pragma(conn, name)),
        ReadRequest::Close { reply } => return Some(reply),
    }
    None
}
