//! Crash reports, S22's launch cut (decision record 0060): [`Crashes`], the handle that records a
//! crash when it happens, queues its report at the next tick, and sends it with counts.
//!
//! - Recording. While counts are saving, a crash writes its report whole, once, under
//!   `<data_dir>/crash/new/`: a panic from the host's panic hook ([`Crashes::prepare_panic`]),
//!   a renderer gone from desktop's webview hook ([`Crashes::record_renderer_gone`]). A native
//!   crash's handler can only append a line to the file [`Crashes::open_native`] opened at start
//!   ([`native_line`]); the next start turns that line into a report. A report is read back
//!   through `parse_crash` and sent as `crash_body` writes it again, so a file changed on disk
//!   goes only while it still passes the schema, and the bytes sent are the app's own.
//! - Hostile files: a file under `crash/` is read only when it is a plain file of at most
//!   `MAX_CRASH_JSON` bytes ([`read_capped`]); the native record is deleted, then made anew
//!   (`create_new`), so a link put in its place is never followed.
//! - The tick ([`send_crash_due`], desktop's 60 s tick): each new report adds `crash` to the week's
//!   tally, then moves into `<data_dir>/crash/queue/` when crash reports may be sent, or is
//!   deleted. At most [`MAX_QUEUED`] wait, each at most [`MAX_AGE_SECS`].
//! - Sending: 1 to 24 h after the crash at random, never within an hour of a counts POST, one at
//!   a time. A 2xx or `400` deletes the report; anything else (the Worker's `429` over its
//!   hourly cap, a `503`, no answer) keeps it for a later try, until its 30 days. Counts turned
//!   off while the POST is in flight cancel it and delete every report.
//! - Off: counts off deletes `<data_dir>/crash/` and stops recording; crash reports locked off by
//!   policy delete every report and still let crashes reach the tally.

mod native;
mod queue;
mod record;

use std::fs::File;
use std::future::Future;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::about::About;
use crate::schema::{MAX_CRASH_JSON, RendererGoneReason, Time};
use futures::future::{Either, select};

use crate::sender::Outcome;
use crate::upload::{Upload, UploadError, UploadPath};
use crate::write;
use crate::{Counts, Mode};

pub use native::{NATIVE_LINE_MAX, native_line};
pub use record::{Pending, area_of_thread, source_file};

use queue::Queue;

/// The most reports waiting to be sent; a crash past them still reaches the tally.
pub const MAX_QUEUED: usize = 5;
/// How long a report waits at most: 30 days.
pub const MAX_AGE_SECS: u64 = 30 * 86_400;
/// The folder under the data folder.
pub const CRASH_DIR: &str = "crash";

/// What the effective settings say, read before each tick and each POST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    /// `privacy.counts` after policy.
    pub counts: bool,
    /// Whether `privacy.crash_reports` is locked off by policy.
    pub locked_off: bool,
}

impl Switch {
    /// Whether reports may be queued and sent: counts on and crash reports not locked off.
    pub const fn may_send(self) -> bool {
        self.counts && !self.locked_off
    }
}

/// One install's crash reports. Cloning it is cheap; every clone is the same reports.
#[derive(Debug, Clone)]
pub struct Crashes {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// `<data_dir>/crash`.
    dir: PathBuf,
    about: About,
    /// Whether a crash is written down: counts saving. Read at crash time, so an atomic.
    recording: AtomicBool,
    /// Whether the window has shown the app.
    launched: AtomicBool,
    /// Whether this process has crashed already: one panic or native report per run.
    crashed: AtomicBool,
    /// Whether a renderer gone was recorded: one such report per run, apart from the above.
    renderer_gone: AtomicBool,
    /// The schedule; never taken at crash time.
    queue: Mutex<Queue>,
}

impl Crashes {
    /// The crash reports of `data_dir`, for an app that says `about`; `seed` spreads the send
    /// times ([`crate::os_seed`] in the app). Records nothing until [`Crashes::set_recording`].
    pub fn open(data_dir: &Path, about: About, seed: u64) -> Crashes {
        Crashes {
            inner: Arc::new(Inner {
                dir: data_dir.join(CRASH_DIR),
                about,
                recording: AtomicBool::new(false),
                launched: AtomicBool::new(false),
                crashed: AtomicBool::new(false),
                renderer_gone: AtomicBool::new(false),
                queue: Mutex::new(Queue::new(seed)),
            }),
        }
    }

    fn queue(&self) -> MutexGuard<'_, Queue> {
        self.inner
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn new_dir(&self) -> PathBuf {
        self.inner.dir.join("new")
    }

    fn queue_dir(&self) -> PathBuf {
        self.inner.dir.join("queue")
    }

    fn native_path(&self) -> PathBuf {
        self.inner.dir.join("native")
    }

    /// Whether crashes are written down: counts saving, which the tick sets.
    pub fn set_recording(&self, on: bool) {
        self.inner.recording.store(on, Ordering::SeqCst);
    }

    /// See [`Crashes::set_recording`]. Safe in a signal handler: one atomic load.
    pub fn recording(&self) -> bool {
        self.inner.recording.load(Ordering::SeqCst)
    }

    /// The window has shown the app: later crashes say `after_launch`.
    pub fn set_launched(&self) {
        self.inner.launched.store(true, Ordering::SeqCst);
    }

    /// See [`Crashes::set_launched`]. Safe in a signal handler: one atomic load.
    pub fn launched(&self) -> bool {
        self.inner.launched.load(Ordering::SeqCst)
    }

    /// Marks this process as crashed; `true` only the first time and only while recording, so
    /// one crash makes one report (a panic's abort is not a second, native one). Safe in a
    /// signal handler: atomics only.
    pub fn begin_crash(&self) -> bool {
        self.recording() && !self.inner.crashed.swap(true, Ordering::SeqCst)
    }

    /// Whether this process has crashed (a panic or native crash began). Signal-safe.
    pub fn crashed(&self) -> bool {
        self.inner.crashed.load(Ordering::SeqCst)
    }

    /// A panic at `now` in `thread`, at `file`'s `line` as the build recorded it: its report,
    /// made on the panicking thread for [`Pending::write`]. `None` when not recording, when
    /// this process crashed already, or when the clock is past the schema's years.
    pub fn prepare_panic(
        &self,
        file: Option<&str>,
        line: u32,
        thread: Option<&str>,
        now: Time,
    ) -> Option<Pending> {
        if !self.begin_crash() {
            return None;
        }
        let report = record::panic(&self.inner.about, self.launched(), file, line, thread, now)?;
        Pending::new(&self.new_dir(), &report, now).ok()
    }

    /// The webview's renderer went: writes its report now, while recording, the first time in
    /// this run only (a renderer that keeps failing makes one report).
    pub fn record_renderer_gone(&self, reason: RendererGoneReason, now: Time) -> io::Result<()> {
        if !self.recording() || self.inner.renderer_gone.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        match record::renderer_gone(&self.inner.about, self.launched(), reason, now) {
            Some(report) => Pending::new(&self.new_dir(), &report, now)?.write(),
            None => Ok(()),
        }
    }

    /// At start, while recording: turns a native crash the last run left into a new report
    /// (the next tick takes it in), then opens the native record empty for this run's handler.
    /// Not recording, it deletes the record and returns `None`. A record past
    /// [`MAX_CRASH_JSON`] bytes, or not a plain file, holds no report.
    pub fn open_native(&self, now: Time) -> io::Result<Option<File>> {
        let path = self.native_path();
        if !self.recording() {
            write::remove_file(&path)?;
            return Ok(None);
        }
        if let Some(bytes) = read_capped(&path)
            && let Some(report) = native::report(&bytes, &self.inner.about, now)
        {
            Pending::new(&self.new_dir(), &report, now)?.write()?;
        }
        write::open_record(&path, &native::header(&self.inner.about)).map(Some)
    }

    /// Deletes every report, new and queued, and the native record.
    pub fn clear(&self) -> io::Result<()> {
        self.queue().forget_all();
        write::remove_dir_all(&self.inner.dir)
    }

    /// The tick's local half: with counts saving, each new report adds `crash` to the tally,
    /// then is queued when `switch` lets reports go (else deleted); reports past their 30 days
    /// or past [`MAX_QUEUED`] go. With counts not saving, everything is deleted.
    pub fn ingest(&self, counts: &Counts, switch: Switch, now: Time) -> io::Result<()> {
        let saving = switch.counts && counts.mode() == Mode::Saving;
        self.set_recording(saving);
        if !saving {
            return self.clear();
        }
        queue::ingest(self, counts, switch.may_send(), now)
    }

    /// The report due at `now`, gzipped, unless a counts POST started within the hour.
    pub fn due(&self, counts: &Counts, now: Time) -> Option<CrashPost> {
        let quiet = counts
            .last_post()
            .is_none_or(|at| now.since(at) >= queue::COUNTS_GAP_SECS);
        if !quiet {
            return None;
        }
        queue::due(self, now)
    }

    /// The answer to the POST of `post`'s report: a 2xx or `400` deletes it, anything else
    /// tries again 1 to 6 hours later.
    pub fn sent(&self, name: &str, outcome: Outcome, now: Time) {
        queue::sent(self, name, outcome, now);
    }

    /// The names of the reports waiting, oldest first.
    pub fn queued(&self) -> Vec<String> {
        queue::names(&self.queue_dir())
    }
}

/// A report to POST to `/v1/crash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashPost {
    /// Its file's name, to pass back to [`Crashes::sent`].
    pub name: String,
    /// The stored report as `parse_crash` read it and `crash_body` wrote it again, gzipped.
    pub gzip_body: Vec<u8>,
}

/// The file at `path` whole, when it is a plain file (not a link) of at most
/// [`MAX_CRASH_JSON`] bytes: none for a bigger one, any other kind or a failed read. Never reads
/// more than one byte past the limit.
pub(crate) fn read_capped(path: &Path) -> Option<Vec<u8>> {
    if !std::fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let limit = u64::try_from(MAX_CRASH_JSON).ok()?.saturating_add(1);
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(limit)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_CRASH_JSON).then_some(bytes)
}

/// The crash half of desktop's tick at `now`: reads `switch`, ingests, and sends the report due,
/// if any, reading `switch` afresh just before the POST. No value from `switch` (the host did
/// not answer) changes nothing and sends nothing. A sent report holds counts' POSTs for an hour
/// after it. Counts turned off while the POST is in flight cancel it (the request is dropped
/// and its answer never read) and delete every report.
pub async fn send_crash_due<F, Fut>(
    crashes: &Crashes,
    counts: &Counts,
    upload: &dyn Upload,
    now: Time,
    switch: F,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = Option<Switch>>,
{
    let Some(first) = switch().await else {
        return;
    };
    // A failed change is tried again on the next tick.
    drop(crashes.ingest(counts, first, now));
    let Some(post) = crashes.due(counts, now) else {
        return;
    };
    let Some(again) = switch().await else {
        return;
    };
    if !again.may_send() {
        drop(crashes.ingest(counts, again, now));
        return;
    }
    let Some(cancelled) = counts.crash_post() else {
        return;
    };
    counts.hold_posts_until(now.plus(queue::COUNTS_GAP_SECS));
    let outcome = match select(upload.post(UploadPath::Crash, post.gzip_body), cancelled).await {
        Either::Left((Ok(status), _)) => Outcome::Status(status),
        Either::Left((Err(UploadError), _)) => Outcome::Failed,
        Either::Right(((), _)) => {
            // Off while in flight: the POST is dropped here, and the reports go with counts.
            drop(crashes.clear());
            return;
        }
    };
    crashes.sent(&post.name, outcome, now);
}
