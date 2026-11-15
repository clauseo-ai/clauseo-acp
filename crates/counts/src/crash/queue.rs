//! The queue on disk and its schedule in memory. A report's file is `<secs>-<id>.json`, `secs`
//! the crash's Unix time, which gives its age and its first try; the file's bytes never change.
//! Every read is [`read_capped`]'s, and a file that does not read whole as a report is deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use crate::random::Rng;
use crate::schema::{Day, Event, Time, crash_body, parse_crash};
use crate::sender::{Outcome, gzip};
use crate::{Counts, write};

use super::{CrashPost, Crashes, MAX_AGE_SECS, MAX_QUEUED, read_capped};

/// No crash report goes within this long after a counts POST, and no counts POST within this
/// long after a crash report.
pub(super) const COUNTS_GAP_SECS: u64 = 3600;
/// A report's first try: 1 hour after the crash, plus up to 23 more at random.
const FIRST_TRY_MIN_SECS: u64 = 3600;
const FIRST_TRY_SPREAD_SECS: u64 = 23 * 3600;
/// After a failed try: 1 hour, plus up to 5 more at random.
const RETRY_MIN_SECS: u64 = 3600;
const RETRY_SPREAD_SECS: u64 = 5 * 3600;

/// The schedule: when each queued report may go next, and which new reports were tallied.
#[derive(Debug)]
pub(super) struct Queue {
    rng: Rng,
    next: BTreeMap<String, Time>,
    /// New reports this process has added to the tally, so one whose move failed is not
    /// counted again at the next tick.
    tallied: BTreeSet<String>,
}

impl Queue {
    pub(super) fn new(seed: u64) -> Queue {
        Queue {
            rng: Rng::new(seed),
            next: BTreeMap::new(),
            tallied: BTreeSet::new(),
        }
    }

    pub(super) fn forget_all(&mut self) {
        self.next.clear();
    }

    /// When `name`, from a crash at `secs`, goes first: 1 to 24 h after the crash, or within
    /// the hour when that has passed (the app was closed).
    fn first_try(&mut self, secs: u64, now: Time) -> Time {
        let at = Time::from_unix_secs(secs)
            .plus(FIRST_TRY_MIN_SECS)
            .plus(self.rng.below(FIRST_TRY_SPREAD_SECS + 1));
        if at < now {
            now.plus(self.rng.below(COUNTS_GAP_SECS + 1))
        } else {
            at
        }
    }
}

/// A report file's crash time, or `None` for any other name.
fn secs_of(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".json")?;
    let (secs, id) = stem.split_once('-')?;
    let fits = !secs.is_empty()
        && secs.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<crate::schema::CrashReportId>().is_ok();
    fits.then(|| secs.parse().ok()).flatten()
}

/// The report files in `dir`, oldest first, and the other files there (a temp file a crash cut
/// short), which go.
fn entries(dir: &Path) -> (Vec<(u64, String)>, Vec<String>) {
    let mut reports = Vec::new();
    let mut other = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return (reports, other);
    };
    for entry in read.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        match secs_of(&name) {
            Some(secs) => reports.push((secs, name)),
            None => other.push(name),
        }
    }
    reports.sort();
    (reports, other)
}

/// The report files' names in `dir`, oldest first.
pub(super) fn names(dir: &Path) -> Vec<String> {
    entries(dir).0.into_iter().map(|(_, name)| name).collect()
}

fn fresh(secs: u64, now: Time) -> bool {
    now.unix_secs().saturating_sub(secs) <= MAX_AGE_SECS
}

/// Keeps the first error.
fn note(first: &mut io::Result<()>, result: io::Result<()>) {
    if let Err(err) = result
        && first.is_ok()
    {
        *first = Err(err);
    }
}

/// See [`Crashes::ingest`]; counts are saving here.
pub(super) fn ingest(
    crashes: &Crashes,
    counts: &Counts,
    may_send: bool,
    now: Time,
) -> io::Result<()> {
    let mut first = Ok(());
    let new_dir = crashes.new_dir();
    let queue_dir = crashes.queue_dir();
    let week = now.day().map(Day::week);
    let (new, other) = entries(&new_dir);
    for name in other {
        note(&mut first, write::remove_file(&new_dir.join(name)));
    }
    let mut queued = names(&queue_dir).len();
    for (secs, name) in new {
        let path = new_dir.join(&name);
        let report = read_capped(&path)
            .zip(week)
            .and_then(|(bytes, week)| parse_crash(&bytes, week).ok());
        if let Some(report) = &report
            && crashes.queue().tallied.insert(name.clone())
        {
            counts.bump(Event::Crash(report.tally_event()));
        }
        let keep = may_send && report.is_some() && fresh(secs, now) && queued < MAX_QUEUED;
        let result = if keep {
            queued += 1;
            write::rename(&path, &queue_dir.join(&name))
        } else {
            write::remove_file(&path)
        };
        note(&mut first, result);
    }
    let (reports, other) = entries(&queue_dir);
    for name in other {
        note(&mut first, write::remove_file(&queue_dir.join(name)));
    }
    for (index, (secs, name)) in reports.into_iter().enumerate() {
        if !may_send || !fresh(secs, now) || index >= MAX_QUEUED {
            crashes.queue().next.remove(&name);
            note(&mut first, write::remove_file(&queue_dir.join(&name)));
        }
    }
    first
}

/// See [`Crashes::due`]: the oldest report whose try has come, read back through `parse_crash`,
/// written again by `crash_body` and gzipped, so only what still passes the schema leaves, in the
/// app's own bytes. A report that no longer reads as one (changed, too big, not a plain file) is
/// deleted.
pub(super) fn due(crashes: &Crashes, now: Time) -> Option<CrashPost> {
    let queue_dir = crashes.queue_dir();
    let week = now.day()?.week();
    for (secs, name) in entries(&queue_dir).0 {
        let next = {
            let mut queue = crashes.queue();
            if let Some(&next) = queue.next.get(&name) {
                next
            } else {
                let next = queue.first_try(secs, now);
                queue.next.insert(name.clone(), next);
                next
            }
        };
        if next > now {
            continue;
        }
        let path = queue_dir.join(&name);
        let body = read_capped(&path)
            .and_then(|bytes| parse_crash(&bytes, week).ok())
            .and_then(|report| crash_body(&report, week).ok())
            .and_then(|json| gzip(&json));
        if let Some(gzip_body) = body {
            return Some(CrashPost { name, gzip_body });
        }
        crashes.queue().next.remove(&name);
        // A failed delete is tried again at the next ingest, which drops it as stale, or this
        // read fails again.
        drop(write::remove_file(&path));
    }
    None
}

/// See [`Crashes::sent`].
pub(super) fn sent(crashes: &Crashes, name: &str, outcome: Outcome, now: Time) {
    let done = matches!(outcome, Outcome::Status(200..=299 | 400));
    let mut queue = crashes.queue();
    if done {
        queue.next.remove(name);
        drop(queue);
        if secs_of(name).is_some() {
            // A failed delete leaves a report the server has; it goes again at most until its 30
            // days, and the Worker takes a report id once.
            drop(write::remove_file(&crashes.queue_dir().join(name)));
        }
    } else {
        let retry = now
            .plus(RETRY_MIN_SECS)
            .plus(queue.rng.below(RETRY_SPREAD_SECS + 1));
        queue.next.insert(name.to_owned(), retry);
    }
}
