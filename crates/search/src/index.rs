//! [`SearchIndex`]: the connection, its writes and the rebuild's progress.

use std::path::Path;

use clauseo_protocol::{MatterId, Seq, StreamId, ThreadId, Timestamp};
use rusqlite::{Connection, ToSql, Transaction, params};

use crate::query::{self, Page, Progress, Query};
use crate::{EntryKind, Error, Kind, OpenState, open};

/// The search index in one folder. Not `Sync`: one thread owns it; [`Interrupter`] is the one
/// thing another thread may use.
pub struct SearchIndex {
    conn: Connection,
    progress: Option<Progress>,
}

/// Stops the query running on a [`SearchIndex`] from another thread. That query returns
/// [`Error::Interrupted`]; a call that is not running a statement is not affected.
pub struct Interrupter(rusqlite::InterruptHandle);

impl Interrupter {
    /// Interrupts the running statement, if any.
    pub fn interrupt(&self) {
        self.0.interrupt();
    }
}

/// Many writes in one transaction, inside [`SearchIndex::batch`].
pub struct Batch<'a> {
    tx: Transaction<'a>,
    done: u64,
}

impl SearchIndex {
    /// Opens `folder/search.sqlite`, or creates it empty with mode `0o600` on Unix. When the
    /// file was missing, did not open, had another format or tokenizer, or holds an unfinished
    /// build, it is emptied and [`OpenState::needs_rebuild`] is true.
    pub fn open(folder: &Path) -> Result<(Self, OpenState), Error> {
        let (conn, needs_rebuild) = open::open(folder)?;
        let index = Self {
            conn,
            progress: None,
        };
        Ok((index, OpenState { needs_rebuild }))
    }

    /// Runs `writes` in one transaction: all of them land, or none when it returns an error.
    /// The conversations it marks with [`Batch::conversation_done`] count toward
    /// [`SearchIndex::progress`] once it commits.
    pub fn batch<T>(
        &mut self,
        writes: impl FnOnce(&mut Batch<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut batch = Batch {
            tx: self.conn.transaction()?,
            done: 0,
        };
        let value = writes(&mut batch)?;
        let done = batch.done;
        batch.tx.commit()?;
        if let Some(progress) = &mut self.progress {
            progress.done = progress.done.saturating_add(done).min(progress.total);
        }
        Ok(value)
    }

    /// Puts a prompt, an answer or a tool title, replacing the row of the same thread and entry.
    pub fn put_entry(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        entry: Seq,
        kind: EntryKind,
        at: Timestamp,
        text: &str,
    ) -> Result<(), Error> {
        self.batch(|b| b.put_entry(matter, thread, entry, kind, at, text))
    }

    /// Puts a conversation's title, replacing the old one; an empty title removes it.
    pub fn put_thread_title(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        at: Timestamp,
        title: &str,
    ) -> Result<(), Error> {
        self.batch(|b| b.put_thread_title(matter, thread, at, title))
    }

    /// Puts a matter's name, replacing the old one; an empty name removes it.
    pub fn put_matter_title(
        &mut self,
        matter: MatterId,
        at: Timestamp,
        title: &str,
    ) -> Result<(), Error> {
        self.batch(|b| b.put_matter_title(matter, at, title))
    }

    /// Replaces every file name of `matter` with `paths`.
    pub fn put_file_names<S: AsRef<str>>(
        &mut self,
        matter: MatterId,
        at: Timestamp,
        paths: impl IntoIterator<Item = S>,
    ) -> Result<(), Error> {
        self.batch(|b| b.put_file_names(matter, at, paths))
    }

    /// Marks a conversation of `matter` archived or not; its rows, and rows put for it later,
    /// follow. The flag is kept even while the conversation has no row.
    pub fn set_archived(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        archived: bool,
    ) -> Result<(), Error> {
        self.batch(|b| b.set_archived(matter, thread, archived))
    }

    /// Deletes the entries and tool titles of `thread` at `entries` (an undone step's), then
    /// checkpoints the WAL.
    pub fn delete_entries(
        &mut self,
        thread: ThreadId,
        entries: impl IntoIterator<Item = Seq>,
    ) -> Result<(), Error> {
        self.batch(|b| b.delete_entries(thread, entries))?;
        self.checkpoint()
    }

    /// Deletes a conversation's rows and title, then checkpoints the WAL so the text does not
    /// linger in it.
    pub fn delete_thread(&mut self, thread: ThreadId) -> Result<(), Error> {
        self.batch(|b| b.delete_thread(thread))?;
        self.checkpoint()
    }

    /// Deletes a matter's rows, its conversations and its name, then checkpoints the WAL.
    pub fn delete_matter(&mut self, matter: MatterId) -> Result<(), Error> {
        self.batch(|b| b.delete_matter(matter))?;
        self.checkpoint()
    }

    /// Starts a rebuild of `total` conversations: marks the file incomplete, so a crash before
    /// [`SearchIndex::finish_rebuild`] reopens with `needs_rebuild`, and counts progress for
    /// [`SearchIndex::progress`] and each [`Page`] until then.
    pub fn start_rebuild(&mut self, total: u64) -> Result<(), Error> {
        open::set_complete(&self.conn, false)?;
        self.progress = Some(Progress { done: 0, total });
        Ok(())
    }

    /// The running rebuild's progress, if one was started and not finished.
    pub fn progress(&self) -> Option<Progress> {
        self.progress
    }

    /// Ends a rebuild: merges the index segments, marks the file complete (so the next open
    /// trusts it) and stops reporting progress.
    pub fn finish_rebuild(&mut self) -> Result<(), Error> {
        self.conn
            .execute_batch("INSERT INTO items_fts(items_fts) VALUES('optimize')")?;
        open::set_complete(&self.conn, true)?;
        self.progress = None;
        Ok(())
    }

    /// Runs `query`.
    pub fn query(&self, query: &Query) -> Result<Page, Error> {
        query::run(&self.conn, query, self.progress)
    }

    /// Every stream's watermark: the last `seq` of it that the index reflects, as
    /// [`Batch::set_watermark`] wrote it. A stream text that does not read is skipped.
    pub fn watermarks(&self) -> Result<Vec<(StreamId, Seq)>, Error> {
        let mut statement = self.conn.prepare("SELECT stream, seq FROM watermarks")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut marks = Vec::new();
        for row in rows {
            let (stream, seq) = row?;
            let seq = u64::try_from(seq).ok().and_then(|seq| Seq::new(seq).ok());
            if let (Ok(stream), Some(seq)) = (stream.parse::<StreamId>(), seq) {
                marks.push((stream, seq));
            }
        }
        Ok(marks)
    }

    /// A handle that interrupts this index's running query from another thread.
    pub fn interrupter(&self) -> Interrupter {
        Interrupter(self.conn.get_interrupt_handle())
    }

    fn checkpoint(&self) -> Result<(), Error> {
        self.conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }
}

impl Batch<'_> {
    /// [`SearchIndex::put_entry`], inside the batch.
    pub fn put_entry(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        entry: Seq,
        kind: EntryKind,
        at: Timestamp,
        text: &str,
    ) -> Result<(), Error> {
        let thread_text = thread.to_string();
        let entry = seq_sql(entry)?;
        self.delete_rows(
            "thread = ?1 AND entry = ?2 AND kind IN (3, 4, 5)",
            &[&thread_text, &entry],
        )?;
        let archived = self.ensure_thread(matter, &thread_text)?;
        let row = Row {
            kind: kind.kind(),
            matter,
            thread: Some(&thread_text),
            entry: Some(entry),
            path: None,
            at,
            archived,
        };
        self.insert(&row, text)
    }

    /// [`SearchIndex::put_thread_title`], inside the batch.
    pub fn put_thread_title(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        at: Timestamp,
        title: &str,
    ) -> Result<(), Error> {
        let thread_text = thread.to_string();
        self.delete_rows("thread = ?1 AND kind = 2", &[&thread_text])?;
        let archived = self.ensure_thread(matter, &thread_text)?;
        let title = clean(title);
        let stored = (!title.trim().is_empty()).then_some(title.as_str());
        self.tx
            .prepare_cached("UPDATE threads SET title = ?2 WHERE thread = ?1")?
            .execute(params![thread_text, stored])?;
        let row = Row {
            kind: Kind::ThreadTitle,
            matter,
            thread: Some(&thread_text),
            entry: None,
            path: None,
            at,
            archived,
        };
        self.insert(&row, &title)
    }

    /// [`SearchIndex::put_matter_title`], inside the batch.
    pub fn put_matter_title(
        &mut self,
        matter: MatterId,
        at: Timestamp,
        title: &str,
    ) -> Result<(), Error> {
        let matter_text = matter.to_string();
        self.delete_rows("matter = ?1 AND kind = 1", &[&matter_text])?;
        let title = clean(title);
        if title.trim().is_empty() {
            self.tx
                .prepare_cached("DELETE FROM matters WHERE matter = ?1")?
                .execute([&matter_text])?;
        } else {
            self.tx
                .prepare_cached(
                    "INSERT INTO matters(matter, title) VALUES (?1, ?2)
                     ON CONFLICT(matter) DO UPDATE SET title = excluded.title",
                )?
                .execute([&matter_text, &title])?;
        }
        let row = Row {
            kind: Kind::MatterTitle,
            matter,
            thread: None,
            entry: None,
            path: None,
            at,
            archived: false,
        };
        self.insert(&row, &title)
    }

    /// [`SearchIndex::put_file_names`], inside the batch.
    pub fn put_file_names<S: AsRef<str>>(
        &mut self,
        matter: MatterId,
        at: Timestamp,
        paths: impl IntoIterator<Item = S>,
    ) -> Result<(), Error> {
        self.delete_rows("matter = ?1 AND kind = 6", &[&matter.to_string()])?;
        for path in paths {
            let path = clean(path.as_ref());
            let row = Row {
                kind: Kind::FileName,
                matter,
                thread: None,
                entry: None,
                path: Some(&path),
                at,
                archived: false,
            };
            self.insert(&row, &path)?;
        }
        Ok(())
    }

    /// [`SearchIndex::set_archived`], inside the batch.
    pub fn set_archived(
        &mut self,
        matter: MatterId,
        thread: ThreadId,
        archived: bool,
    ) -> Result<(), Error> {
        let thread_text = thread.to_string();
        self.ensure_thread(matter, &thread_text)?;
        self.tx
            .prepare_cached("UPDATE threads SET archived = ?2 WHERE thread = ?1")?
            .execute(params![thread_text, archived])?;
        self.tx
            .prepare_cached("UPDATE items SET archived = ?2 WHERE thread = ?1")?
            .execute(params![thread_text, archived])?;
        Ok(())
    }

    /// [`SearchIndex::delete_entries`], inside the batch (without the WAL checkpoint).
    pub fn delete_entries(
        &mut self,
        thread: ThreadId,
        entries: impl IntoIterator<Item = Seq>,
    ) -> Result<(), Error> {
        let thread_text = thread.to_string();
        for entry in entries {
            self.delete_rows(
                "thread = ?1 AND entry = ?2 AND kind IN (3, 4, 5)",
                &[&thread_text, &seq_sql(entry)?],
            )?;
        }
        Ok(())
    }

    /// [`SearchIndex::delete_thread`], inside the batch (without the WAL checkpoint).
    pub fn delete_thread(&mut self, thread: ThreadId) -> Result<(), Error> {
        let thread_text = thread.to_string();
        self.delete_rows("thread = ?1", &[&thread_text])?;
        self.tx
            .prepare_cached("DELETE FROM threads WHERE thread = ?1")?
            .execute([&thread_text])?;
        Ok(())
    }

    /// [`SearchIndex::delete_matter`], inside the batch (without the WAL checkpoint).
    pub fn delete_matter(&mut self, matter: MatterId) -> Result<(), Error> {
        let matter_text = matter.to_string();
        self.delete_rows("matter = ?1", &[&matter_text])?;
        self.tx
            .prepare_cached("DELETE FROM threads WHERE matter = ?1")?
            .execute([&matter_text])?;
        self.tx
            .prepare_cached("DELETE FROM matters WHERE matter = ?1")?
            .execute([&matter_text])?;
        Ok(())
    }

    /// Records that the index reflects `stream` up to `seq`, in the same transaction as the
    /// batch's rows; `None` forgets the stream's watermark.
    pub fn set_watermark(&mut self, stream: StreamId, seq: Option<Seq>) -> Result<(), Error> {
        let stream = stream.to_string();
        match seq {
            Some(seq) => {
                self.tx
                    .prepare_cached(
                        "INSERT INTO watermarks(stream, seq) VALUES (?1, ?2)
                         ON CONFLICT(stream) DO UPDATE SET seq = excluded.seq",
                    )?
                    .execute(params![stream, seq_sql(seq)?])?;
            }
            None => {
                self.tx
                    .prepare_cached("DELETE FROM watermarks WHERE stream = ?1")?
                    .execute([stream])?;
            }
        }
        Ok(())
    }

    /// Counts one conversation toward the rebuild's progress when the batch commits.
    pub fn conversation_done(&mut self) {
        self.done = self.done.saturating_add(1);
    }

    /// The thread's row, made if missing; returns whether it is archived.
    fn ensure_thread(&self, matter: MatterId, thread: &str) -> Result<bool, Error> {
        self.tx
            .prepare_cached(
                "INSERT INTO threads(thread, matter) VALUES (?1, ?2)
                 ON CONFLICT(thread) DO UPDATE SET matter = excluded.matter",
            )?
            .execute([thread, &matter.to_string()])?;
        Ok(self
            .tx
            .prepare_cached("SELECT archived FROM threads WHERE thread = ?1")?
            .query_row([thread], |row| row.get(0))?)
    }

    /// Deletes the rows `filter` selects from the FTS index (with the exact text it indexed)
    /// and from `items`.
    fn delete_rows(&self, filter: &str, values: &[&dyn ToSql]) -> Result<(), Error> {
        self.tx
            .prepare_cached(&format!(
                "INSERT INTO items_fts(items_fts, rowid, body)
                 SELECT 'delete', id, body FROM items WHERE {filter}"
            ))?
            .execute(values)?;
        self.tx
            .prepare_cached(&format!("DELETE FROM items WHERE {filter}"))?
            .execute(values)?;
        Ok(())
    }

    /// Inserts one row and indexes `body`; text with no characters is not stored.
    fn insert(&self, row: &Row<'_>, body: &str) -> Result<(), Error> {
        let body = clean(body);
        if body.trim().is_empty() {
            return Ok(());
        }
        self.tx
            .prepare_cached(
                "INSERT INTO items(kind, matter, thread, entry, path, at, archived, body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?
            .execute(params![
                row.kind.code(),
                row.matter.to_string(),
                row.thread,
                row.entry,
                row.path,
                row.at.as_millis(),
                row.archived,
                body,
            ])?;
        let id = self.tx.last_insert_rowid();
        self.tx
            .prepare_cached("INSERT INTO items_fts(rowid, body) VALUES (?1, ?2)")?
            .execute(params![id, body])?;
        Ok(())
    }
}

/// One row's columns other than its text.
struct Row<'a> {
    kind: Kind,
    matter: MatterId,
    thread: Option<&'a str>,
    entry: Option<i64>,
    path: Option<&'a str>,
    at: Timestamp,
    archived: bool,
}

/// `text` without the two snippet markers, U+0002 and U+0003.
fn clean(text: &str) -> String {
    text.replace(['\u{2}', '\u{3}'], "")
}

fn seq_sql(seq: Seq) -> Result<i64, Error> {
    i64::try_from(seq.get()).map_err(|error| Error::Corrupt(error.to_string()))
}
