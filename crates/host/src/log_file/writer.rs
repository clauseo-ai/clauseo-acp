//! The writer thread's side: the [`Sink`] seam (the files on disk, or a test's stand-in), and
//! [`Writer`], which batches lines, rotates by size, counts what it drops and backs off after a
//! failure. Nothing here ever panics or waits on a caller.

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use super::{LOG_FILE, Shared, stamp};
use crate::write;

/// How long the writer waits after a failed write or rotation before it tries the disk again;
/// lines that come in between are dropped and counted.
pub(super) const BACKOFF: Duration = Duration::from_secs(5);

/// At most this much of a batch is held before it is written.
const BATCH_BYTES: usize = 64 * 1024;

/// Where lines go: `host.log` and its rotations, or a test's stand-in.
pub(crate) trait Sink: Send + 'static {
    /// The bytes in the current file: read at the start, so a restart keeps counting toward
    /// the cap, and after each write, so a partial write or a file made again counts what is
    /// really there.
    fn len(&mut self) -> u64;
    /// Appends `bytes` to the current file; an error may leave some of them written.
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// `host.1.log` becomes `host.2.log` (the old one goes), `host.log` becomes `host.1.log`,
    /// and the next write starts a new `host.log`. An error leaves the current file in place.
    fn rotate(&mut self) -> io::Result<()>;
}

/// `host.log` in the log folder, opened for append on first use and after each rotation.
pub(crate) struct FileSink {
    dir: PathBuf,
    file: Option<File>,
}

impl FileSink {
    /// The sink in `dir`, which must exist. Opens nothing yet.
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir, file: None }
    }

    fn name(&self, index: u8) -> PathBuf {
        match index {
            0 => self.dir.join(LOG_FILE),
            n => self.dir.join(format!("host.{n}.log")),
        }
    }

    /// The open `host.log`, opened (and its folder made again, if someone deleted it) when
    /// none is open.
    fn file(&mut self) -> io::Result<&mut File> {
        if self.file.is_none() {
            let path = self.name(0);
            let opened = match write::open_append(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    write::create_dir_all(&self.dir).and_then(|()| write::open_append(&path))
                }
                opened => opened,
            };
            self.file = Some(opened?);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("no log file"))
    }
}

impl Sink for FileSink {
    fn len(&mut self) -> u64 {
        std::fs::metadata(self.name(0)).map_or(0, |meta| meta.len())
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        // A `host.log` deleted while open (with its folder, or alone) is opened again; on Unix
        // the old handle would go on writing to a file no one can see.
        if self.file.is_some() && !self.name(0).exists() {
            self.file = None;
        }
        let written = self.file()?.write_all(bytes);
        if written.is_err() {
            // The next try opens the file again: it may have gone, or its drive come back.
            self.file = None;
        }
        written
    }

    fn rotate(&mut self) -> io::Result<()> {
        // Closed first: Windows refuses to rename a file this process holds open.
        self.file = None;
        let (current, first, second) = (self.name(0), self.name(1), self.name(2));
        if first.exists() {
            write::rename_file(&first, &second)?;
        }
        write::rename_file(&current, &first)
    }
}

/// The writer thread's state.
pub(crate) struct Writer<S> {
    sink: S,
    /// The size at which `host.log` rotates.
    cap: u64,
    /// The bytes in the current file.
    size: u64,
    /// The lines and bytes taken but not yet written.
    batch: Vec<u8>,
    batch_lines: u64,
    /// The drops the batch's `dropped N lines` names: taken off the count once it is written.
    batch_noted: u64,
    /// Until when the disk is left alone after a failure (on [`Writer::now`]'s clock).
    paused_until: Option<Duration>,
    shared: Arc<Shared>,
    /// Monotonic time, for the backoff.
    now: Box<dyn Fn() -> Duration + Send>,
}

impl<S: Sink> Writer<S> {
    pub(crate) fn new(
        mut sink: S,
        cap: u64,
        shared: Arc<Shared>,
        now: Box<dyn Fn() -> Duration + Send>,
    ) -> Self {
        let size = sink.len();
        Self {
            sink,
            cap,
            size,
            batch: Vec::new(),
            batch_lines: 0,
            batch_noted: 0,
            paused_until: None,
            shared,
            now,
        }
    }

    /// Writes what comes on `lines` until every sender is gone. Each wake takes everything
    /// queued, writes it in batches, then tells a waiting guard how far it got.
    pub(crate) fn run(mut self, lines: &Receiver<Vec<u8>>) {
        while let Ok(line) = lines.recv() {
            let mut handled = 1;
            self.take(&line);
            while let Ok(line) = lines.try_recv() {
                handled += 1;
                self.take(&line);
                if self.batch.len() >= BATCH_BYTES {
                    self.flush();
                }
            }
            self.flush();
            self.shared.handled(handled);
        }
        self.flush();
    }

    fn paused(&self) -> bool {
        self.paused_until.is_some_and(|until| (self.now)() < until)
    }

    fn pause(&mut self) {
        self.paused_until = Some((self.now)() + BACKOFF);
    }

    fn drop_lines(&self, lines: u64) {
        self.shared.dropped.fetch_add(lines, Ordering::Relaxed);
    }

    /// Adds `line` to the batch, rotating first when it would take the file past the cap. A
    /// line that cannot go (paused, rotation failed, or longer than the cap) is counted.
    fn take(&mut self, line: &[u8]) {
        if self.paused() || line.len() as u64 > self.cap {
            self.drop_lines(1);
            return;
        }
        if self.batch.is_empty() {
            self.note_drops();
        }
        if !self.room_for(line.len()) {
            self.drop_lines(1);
            return;
        }
        self.batch.extend_from_slice(line);
        self.batch_lines += 1;
    }

    /// Puts `dropped N lines` first in a new batch when lines were dropped since the last.
    /// The count keeps them until the note is written, so it never reads low while a write
    /// hangs or fails.
    fn note_drops(&mut self) {
        let dropped = self.shared.dropped.load(Ordering::Relaxed);
        if dropped == 0 {
            return;
        }
        let note = format!(
            "{} WARN clauseo_host::log_file: dropped {dropped} lines\n",
            stamp()
        );
        if self.room_for(note.len()) {
            self.batch.extend_from_slice(note.as_bytes());
            self.batch_noted = dropped;
        }
    }

    /// Whether `bytes` more fit in the current file, rotating when they would not.
    fn room_for(&mut self, bytes: usize) -> bool {
        let needed = self.size + (self.batch.len() + bytes) as u64;
        if needed <= self.cap {
            return true;
        }
        self.flush();
        // The flush read the file's real length: a file made again may have room after all.
        if self.size + bytes as u64 <= self.cap {
            return true;
        }
        if self.paused() {
            return false;
        }
        if self.size > 0 {
            if self.sink.rotate().is_err() {
                // Held open elsewhere (Windows) or the folder refuses: stop at the cap, and
                // try again after the backoff.
                self.pause();
                return false;
            }
            self.size = 0;
        }
        self.size + (self.batch.len() + bytes) as u64 <= self.cap
    }

    /// Writes the batch. A failed write drops its lines, counted, and pauses the disk.
    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        let lines = std::mem::take(&mut self.batch_lines);
        let noted = std::mem::take(&mut self.batch_noted);
        if self.paused() {
            self.drop_lines(lines);
        } else {
            let written = self.sink.write(&self.batch);
            // What is really in the file now: all of the batch, part of it, or a file made
            // again after it was deleted.
            self.size = self.sink.len();
            if written.is_ok() {
                self.shared.dropped.fetch_sub(noted, Ordering::Relaxed);
            } else {
                self.drop_lines(lines);
                self.pause();
            }
        }
        self.batch.clear();
    }
}
