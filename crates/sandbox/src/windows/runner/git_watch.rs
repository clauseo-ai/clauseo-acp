//! The outer runner's live watch for a git repository made in a folder the agent can write (S15
//! tier B): the matter's, and its notes folder. The agent can rename a lawyer's own folder or
//! file to `.git`, fill one with a bare repository's `HEAD`, `objects` and `refs`, or set a
//! junction on an empty folder; it stays the lawyer's, so git's ownership check trusts it, and
//! the lawyer's VS Code polling (or the lawyer's own git) would run its `core.fsmonitor` or
//! hooks as the lawyer before the after-session set-aside moves it. The outer runs as the lawyer
//! for the whole session, so it watches the folders itself, always on and depending on nothing
//! IT can switch off (no `SandboxWatch`, no ETW):
//!
//! - `ReadDirectoryChangesExW` on each folder (`os.rs`), recursive, for names made and renamed
//!   and for attributes and writes ([`records`]: `FILE_ACTION_ADDED`,
//!   `FILE_ACTION_RENAMED_NEW_NAME`, and `FILE_ACTION_MODIFIED` of an item that is a reparse
//!   point now), armed before the inner runner starts. Each goes to the set-aside's live check
//!   (`set_aside/live.rs`), which moves an unknown `.git`, bare repository, `HEAD` or link out
//!   at once, by the after-session pass's own move, manifest and lock.
//! - Kill first: the first item found ends the session (the job ended and emptied, as every
//!   end path does) before anything moves, so nothing races the moves. So does a scan past
//!   `set_aside::SCAN_BUDGET`, [`OVERFLOWS`] overflows in a row, or items reported gone
//!   [`GONE_LIMIT`] times in all: an agent that keeps the watch busy ends its own session. The
//!   lawyer is told why in plain words, one line for each cause ([`Cause::notice`]). When the
//!   end can't confirm the agent's job is empty, nothing is moved while it runs; the check
//!   after the session moves it, said once.
//! - The budget doesn't depend on the check keeping time between items: the watch marks each
//!   check busy ([`Pulse`]), and a watchdog thread ends the session when one check stays busy
//!   past `set_aside::SCAN_BUDGET` (a call blocked by an agent's oplock, say): [`watchdog`].
//! - An overflow (`ERROR_NOTIFY_ENUM_DIR`, an empty or unreadable buffer) rescans the whole
//!   folder and reads on. A watch that can't be armed refuses the launch (`outer/watch.rs`),
//!   and one that breaks while the agent runs ends the session: nothing stands in for it.
//! - A folder renamed or moved in and gone before its scan counts as gone; one made just now
//!   (`mkdir`, by its creation time) only when its file ID left another watched place, the
//!   agent's state and temp folders included ([`pairs`]).
//! - An item that can't be moved (the agent holding a handle to it, say) is tried again once
//!   the session ended: fail closed.
//! - Once the job is empty, the watch is stopped and rescans its folder one last time. When the
//!   watch ended the session and emptying the job failed, that rescan moves nothing either and
//!   leaves what it finds for the check after the session.
//! - Each item moved gets two records on the host's denial pipe (`denial_pipe`, never stderr,
//!   which carries the agent's lines too): the lawyer's notice, where it went
//!   ([`SetAsideWhat::notice`]), and the [`Denial::SetAside`] for the session's denial sink
//!   ([`Session::record`]). The manifest lists it, as it lists the after-session pass's. That
//!   pass stays the backstop.

use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) use self::pairs::Paired;
#[cfg(windows)]
pub(crate) use self::pairs::Pairs;
pub(crate) use self::records::Changes;
#[cfg(windows)]
pub(crate) use self::records::changes;
pub(crate) use self::watchdog::{Dog, Pulse, watchdog};
use crate::windows::denial_pipe::Record;
use crate::windows::set_aside::{Ending, Failed, Hooks, Live};
#[cfg(doc)]
use crate::{Denial, SetAsideWhat};

#[cfg(windows)]
pub(super) mod os;
mod pairs;
mod records;
mod watchdog;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;

/// How many overflows in a row end the session.
pub(crate) const OVERFLOWS: u32 = 3;
/// How many items, in all, may be reported or found gone before the session ends.
pub(crate) const GONE_LIMIT: u32 = 3;
/// How often a broken watch, having ended the session, looks for its stop.
const STOP_POLL: Duration = Duration::from_millis(250);
/// How many more tries an item the watch couldn't move gets once the session ended.
const RETRIES: u32 = 5;
/// The pause between two of those tries, while the ended agent's handles close.
const RETRY_PAUSE: Duration = Duration::from_millis(200);

/// What starts each line the outer says on its stderr: log lines only, which nothing reads back.
const SAID: &str = "clauseo-sandbox-run: ";

/// What the watch saw next.
#[derive(Debug)]
pub(crate) enum Next {
    /// Names made or renamed, and items turned into reparse points, relative to the folder.
    Changed(Changes),
    /// More changed than the watch could hold: what changed is unknown.
    Overflow,
    /// The watch failed: it reads nothing more.
    Broken(io::Error),
    /// The session ended: the watch is to stop.
    Stopped,
}

/// A watch armed on the folder.
pub(crate) trait Source {
    /// Blocks until something changed or the watch is stopped.
    fn next(&mut self) -> Next;
}

/// The set-aside's live check (`set_aside::GitGuard` on Windows).
pub(crate) trait Guard {
    /// Checks what the watch reported.
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live;
    /// Scans the whole folder.
    fn rescan(&self, hooks: &impl Hooks) -> Live;
    /// Tries again what a check couldn't move.
    fn retry(&self, failed: &[Failed], hooks: &impl Hooks) -> Live;
}

/// What the watch asks of the session.
pub(crate) trait Session {
    /// Ends the session by the outer's own end path. Whether the agent's job is truly empty
    /// once it returns: only then does the watch move anything while the session runs.
    fn end(&self) -> bool;
    /// One line on the outer's stderr, for the log: never a notice or a set-aside.
    fn say(&self, line: &str);
    /// One record for the host, on its denial pipe: the lawyer's notices and the set-asides.
    fn record(&self, record: &Record);
    /// Waits up to `wait`; `true`, early, once the watch is stopped.
    fn pause(&self, wait: Duration) -> bool;
    /// Waits `wait`, stopped or not: between the tries after the session ended.
    fn settle(&self, wait: Duration);
    /// A monotonic reading, for the scans' budget and the watchdog.
    fn now(&self) -> Duration;
    /// What the watch shares with its watchdog: each check marked busy while it runs.
    fn pulse(&self) -> &Pulse;
    /// What the watch shares with the other watches, to pair a folder made just now with its
    /// file ID leaving another place (`pairs.rs`); with none, such a folder never counts gone.
    fn paired(&self) -> Option<&Paired> {
        None
    }
}

/// Why the watch ended the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// A git repository, or a link out, appeared while the agent ran.
    Found,
    /// A scan ran past `set_aside::SCAN_BUDGET` while the watch was armed.
    Slow,
    /// [`OVERFLOWS`] overflows in a row.
    Overflows,
    /// One path gone [`GONE_LIMIT`] times before it could be moved.
    Gone,
    /// An item found couldn't be moved while the agent ran.
    Stuck,
    /// The watch's thread panicked or ended before it was stopped (`outer/watch.rs`).
    WatchFailed,
    /// The watch broke while the agent ran ([`Next::Broken`]).
    Broken,
    /// One check stayed busy past `set_aside::SCAN_BUDGET` (the watchdog, [`watchdog`]).
    Stalled,
}

impl Cause {
    /// The lawyer's notice, in plain words: why the session stopped.
    pub(crate) fn notice(self) -> &'static str {
        match self {
            Cause::Found => {
                "Clauseo stopped this session because the AI tool tried to put a code repository, \
                 or a link leading outside the matter, in the matter."
            }
            Cause::Slow => {
                "Clauseo stopped this session because the AI tool added more to the matter than \
                 Clauseo could check in time. Nothing was lost."
            }
            Cause::Overflows => {
                "Clauseo stopped this session because the matter changed faster than Clauseo \
                 could check it (very many files at once). Nothing was lost."
            }
            Cause::Gone => {
                "Clauseo stopped this session because the AI tool kept moving an item before \
                 Clauseo could check it. Nothing was lost."
            }
            Cause::Stuck => {
                "Clauseo stopped this session because something the AI tool made in the matter \
                 couldn't be moved aside while it ran. Clauseo tries again now it has stopped."
            }
            Cause::WatchFailed => {
                "Clauseo stopped this session because its watch on the matter stopped working. \
                 Nothing was lost."
            }
            Cause::Broken => {
                "Clauseo stopped this session because it could no longer watch the matter for \
                 changes. Nothing was lost."
            }
            Cause::Stalled => {
                "Clauseo stopped this session because checking the matter got stuck. Nothing was \
                 lost."
            }
        }
    }

    /// The same, for the log: no path.
    fn log(self) -> &'static str {
        match self {
            Cause::Found => "a git repository or a link out appeared while the agent ran",
            Cause::Slow => "a scan of the folder ran past its time",
            Cause::Overflows => "overflows in a row",
            Cause::Gone => "an item kept going before it could be set aside",
            Cause::Stuck => "an item couldn't be set aside",
            Cause::WatchFailed => "the watch failed",
            Cause::Broken => "the watch broke while the agent ran",
            Cause::Stalled => "a check stayed busy past its budget",
        }
    }
}

/// The watch's own count of what ends the session.
struct Ender<'a, S: Session> {
    session: &'a S,
    ended: Cell<bool>,
    /// What the end said of the agent's job: empty, so moves may go on.
    empty: Cell<bool>,
    overflows: Cell<u32>,
    /// Items gone before they could be moved, in all.
    gone: Cell<u32>,
    /// Whether the items left for the check after the session were said.
    left_said: Cell<bool>,
}

impl<S: Session> Ender<'_, S> {
    /// Ends the session the first time, telling the lawyer why in plain words (unless the
    /// watchdog ended it first and said its own). Whether the agent's job is empty.
    fn stop(&self, cause: Cause) -> bool {
        if !self.ended.replace(true) {
            tracing::warn!(cause = cause.log(), "the git watch is ending the session");
            if self.session.pulse().end() {
                self.session.record(&Record::notice(cause.notice()));
            }
            self.empty.set(self.session.end());
        }
        self.empty.get()
    }
}

impl<S: Session> Hooks for Ender<'_, S> {
    fn end(&self, why: Ending) -> bool {
        self.stop(match why {
            Ending::Found => Cause::Found,
            Ending::Slow => Cause::Slow,
        })
    }

    fn now(&self) -> Duration {
        self.session.now()
    }
}

/// Runs `check` with the watch marked busy for its watchdog ([`Pulse`]).
fn busy<T, S: Session>(session: &S, check: impl FnOnce() -> T) -> T {
    let pulse = session.pulse();
    pulse.busy(session.now());
    let done = check();
    pulse.idle();
    done
}

/// Runs the watch, armed before the agent started, until it is stopped, then rescans once.
pub(crate) fn watch<S: Source>(mut source: S, guard: &impl Guard, session: &impl Session) {
    let ender = Ender {
        session,
        ended: Cell::new(false),
        empty: Cell::new(false),
        overflows: Cell::new(0),
        gone: Cell::new(0),
        left_said: Cell::new(false),
    };
    keep_watching(&mut source, guard, &ender);
    // One last look, with nothing left to end, and nothing for the watchdog. The job is empty
    // unless the watch ended it and emptying it failed: then this look moves nothing either and
    // leaves what it finds (`Live::left`) for the check after the session.
    session.pulse().end();
    let emptied = !ender.ended.replace(true) || ender.empty.get();
    ender.empty.set(emptied);
    act(&busy(session, || guard.rescan(&ender)), guard, &ender);
}

/// [`watch`]'s loop, until the watch is stopped. A watch that breaks ends the session and
/// waits for its stop: nothing stands in for it.
fn keep_watching<S: Source, T: Session>(source: &mut S, guard: &impl Guard, ender: &Ender<'_, T>) {
    let session = ender.session;
    loop {
        match source.next() {
            Next::Changed(changes) => {
                ender.overflows.set(0);
                let paired = session.paired();
                if let Some(paired) = paired {
                    paired.note(session.now(), &changes.left);
                }
                let mut live = busy(session, || guard.changed(&changes, ender));
                pairs::promote(&mut live, &changes.fresh_folders, paired, session);
                act(&live, guard, ender);
            }
            Next::Overflow => overflowed(guard, ender),
            Next::Broken(error) => {
                session.say(&format!(
                    "{SAID}git watch: the watch failed ({}); ending the session",
                    code(&error)
                ));
                ender.stop(Cause::Broken);
                while !session.pause(STOP_POLL) {}
                return;
            }
            Next::Stopped => return,
        }
    }
}

/// A watch on a folder the agent can write that only feeds the pairing (`pairs.rs`): the
/// agent's state and temp folders. It moves nothing and ends nothing; one that breaks or is
/// stopped just ends, and an overflow loses what it would have noted.
pub(crate) fn feed(mut source: impl Source, paired: &Paired, now: impl Fn() -> Duration) {
    loop {
        match source.next() {
            Next::Changed(changes) => paired.note(now(), &changes.left),
            Next::Overflow => {}
            Next::Broken(error) => {
                tracing::warn!(
                    code = code(&error),
                    "a pairing watch broke; it notes no more"
                );
                return;
            }
            Next::Stopped => return,
        }
    }
}

/// An overflow: said, counted (past [`OVERFLOWS`] in a row the session ends), then a rescan.
fn overflowed<T: Session>(guard: &impl Guard, ender: &Ender<'_, T>) {
    let count = ender.overflows.get().saturating_add(1);
    ender.overflows.set(count);
    ender.session.say(&format!(
        "{SAID}git watch: too many changes at once; rescanning the folder"
    ));
    if count >= OVERFLOWS {
        ender.stop(Cause::Overflows);
    }
    act(&busy(ender.session, || guard.rescan(ender)), guard, ender);
}

/// Tells the lawyer of each item `live` moved and counts what went before it could be moved;
/// when any couldn't be moved, ends the session (if the move's own kill didn't), then tries
/// those again.
fn act<T: Session>(live: &Live, guard: &impl Guard, ender: &Ender<'_, T>) {
    let session = ender.session;
    tell(live, session);
    count_gone(&live.gone, ender);
    if !live.left.is_empty() && !ender.left_said.replace(true) {
        session.say(&format!(
            "{SAID}git watch: {} item(s) not set aside while the agent's processes may still \
             run; the check after the session sets them aside",
            live.left.len()
        ));
    }
    if live.failed.is_empty() {
        return;
    }
    for failed in &live.failed {
        session.say(&format!(
            "{SAID}git watch: an item couldn't be set aside ({}); trying again once the session \
             ended",
            failed.failure
        ));
    }
    ender.stop(Cause::Stuck);
    let mut left = live.failed.clone();
    for _ in 0..RETRIES {
        let again = busy(session, || guard.retry(&left, ender));
        tell(&again, session);
        left = again.failed;
        if left.is_empty() {
            return;
        }
        session.settle(RETRY_PAUSE);
    }
    session.say(&format!(
        "{SAID}git watch: {} item(s) still couldn't be set aside once the session ended; the \
         check after the session tries again",
        left.len()
    ));
}

/// Counts the items gone, across every path: [`GONE_LIMIT`] in all ends the session, so an
/// agent can't keep new names coming and going (`a1/.git`, `a2/.git`, ...).
fn count_gone<T: Session>(gone: &[PathBuf], ender: &Ender<'_, T>) {
    let count = u32::try_from(gone.len())
        .unwrap_or(u32::MAX)
        .saturating_add(ender.gone.get());
    ender.gone.set(count);
    if !gone.is_empty() && count >= GONE_LIMIT {
        ender.stop(Cause::Gone);
    }
}

/// Two records for each item moved: the lawyer's notice, where it went, and the set-aside the
/// host makes a [`Denial::SetAside`] of for the session's denial sink, which tells the agent.
fn tell(live: &Live, session: &impl Session) {
    for moved in &live.moved {
        tracing::info!(id = moved.entry.id, "the git watch set an item aside");
        let to = &moved.entry.to;
        let notice = moved.what.notice_by(&moved.path, to, moved.by_agent);
        session.record(&Record::notice(notice));
        session.record(&Record::set_aside(&moved.path, to, moved.what));
    }
}

/// An error's code, or its kind: never a path.
fn code(error: &io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |code| code.to_string())
}
