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
//!   `set_aside::SCAN_BUDGET`, [`OVERFLOWS`] overflows in a row, or one path reported gone
//!   [`GONE_LIMIT`] times: an agent that keeps the watch busy ends its own session. The
//!   lawyer is told why in plain words, one line for each cause ([`Cause::notice`]).
//! - An overflow (`ERROR_NOTIFY_ENUM_DIR`, an empty or unreadable buffer) rescans the whole
//!   folder and reads on. A watch that breaks, or can't be armed, rescans every
//!   [`RESCAN_EVERY`] and tries to arm again, saying so once (degraded).
//! - An item that can't be moved (the agent holding a handle to it, say) is tried again once
//!   the session ended: fail closed.
//! - Once the job is empty, the watch is stopped and rescans its folder one last time.
//! - Each item moved gets two lines on the outer's stderr: the lawyer's notice, where it went
//!   ([`SetAsideWhat::notice`]), and one the host reads back as a [`Denial::SetAside`]
//!   ([`denial_from_line`]). The manifest lists it, as it lists the after-session pass's. That
//!   pass stays the backstop.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) use self::records::Changes;
#[cfg(windows)]
pub(crate) use self::records::changes;
use crate::windows::set_aside::{Ending, Failed, Hooks, Live};
use crate::{Denial, SetAsideWhat};

#[cfg(windows)]
pub(super) mod os;
mod records;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;

/// How often the folder is rescanned while the watch can't be armed, or after it broke.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(2);
/// How many overflows in a row end the session.
pub(crate) const OVERFLOWS: u32 = 3;
/// How many times one path may be reported or found gone before the session ends.
pub(crate) const GONE_LIMIT: u32 = 3;
/// How many more tries an item the watch couldn't move gets once the session ended.
const RETRIES: u32 = 5;
/// The pause between two of those tries, while the ended agent's handles close.
const RETRY_PAUSE: Duration = Duration::from_millis(200);

/// What starts each line the outer says on its stderr.
const SAID: &str = "clauseo-sandbox-run: ";
/// What starts the line the host reads back as a [`Denial::SetAside`], after [`SAID`]; its
/// fields follow, each after a tab, which no Windows name holds.
const SET_ASIDE: &str = "set aside";

/// What the watch saw next.
#[derive(Debug)]
pub(crate) enum Next {
    /// Names made or renamed, and items turned into reparse points, relative to the folder.
    Changed(Changes),
    /// More changed than the watch could hold: what changed is unknown.
    Overflow,
    /// The watch failed and must be armed again.
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
    /// Ends the session: the agent's job ended and emptied, by the outer's own end path,
    /// before it returns.
    fn end(&self);
    /// One line on the outer's stderr.
    fn say(&self, line: &str);
    /// Waits up to `wait`; `true`, early, once the watch is stopped.
    fn pause(&self, wait: Duration) -> bool;
    /// Waits `wait`, stopped or not: between the tries after the session ended.
    fn settle(&self, wait: Duration);
    /// A monotonic reading, for the scans' budget.
    fn now(&self) -> Duration;
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
    /// A scan ran past its budget while the watch couldn't be armed (rescans stood in).
    SlowUnwatched,
    /// The watch's thread panicked or ended before it was stopped (`outer/watch.rs`).
    WatchFailed,
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
            Cause::SlowUnwatched => {
                "Clauseo stopped this session because it couldn't watch the matter for changes, \
                 and checking it another way took too long. Nothing was lost."
            }
            Cause::WatchFailed => {
                "Clauseo stopped this session because its watch on the matter stopped working. \
                 Nothing was lost."
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
            Cause::SlowUnwatched => "a rescan ran past its time while the watch wasn't armed",
            Cause::WatchFailed => "the watch failed",
        }
    }
}

/// The watch's own count of what ends the session.
struct Ender<'a, S: Session> {
    session: &'a S,
    ended: Cell<bool>,
    overflows: Cell<u32>,
    gone: std::cell::RefCell<BTreeMap<PathBuf, u32>>,
    /// The watch can't be armed, or broke, and rescans stand in.
    degraded: Cell<bool>,
}

impl<S: Session> Ender<'_, S> {
    /// Ends the session the first time, telling the lawyer why in plain words.
    fn stop(&self, cause: Cause) {
        if !self.ended.replace(true) {
            tracing::warn!(cause = cause.log(), "the git watch is ending the session");
            self.session.say(&format!("{SAID}{}", cause.notice()));
            self.session.end();
        }
    }
}

impl<S: Session> Hooks for Ender<'_, S> {
    fn end(&self, why: Ending) {
        self.stop(match why {
            Ending::Found => Cause::Found,
            Ending::Slow if self.degraded.get() => Cause::SlowUnwatched,
            Ending::Slow => Cause::Slow,
        });
    }

    fn now(&self) -> Duration {
        self.session.now()
    }
}

/// Runs the watch until it is stopped, then rescans once: `first` is the watch armed before the
/// agent started, `arm` arms it again.
pub(crate) fn watch<S: Source>(
    first: io::Result<S>,
    mut arm: impl FnMut() -> io::Result<S>,
    guard: &impl Guard,
    session: &impl Session,
) {
    let ender = Ender {
        session,
        ended: Cell::new(false),
        overflows: Cell::new(0),
        gone: std::cell::RefCell::new(BTreeMap::new()),
        degraded: Cell::new(false),
    };
    keep_watching(first, &mut arm, guard, &ender);
    // The job is empty: one last look, with nothing left to end.
    ender.ended.set(true);
    act(&guard.rescan(&ender), guard, &ender);
}

/// [`watch`]'s loop, until the watch is stopped.
fn keep_watching<S: Source, T: Session>(
    first: io::Result<S>,
    arm: &mut impl FnMut() -> io::Result<S>,
    guard: &impl Guard,
    ender: &Ender<'_, T>,
) {
    let session = ender.session;
    let mut armed = first;
    let degraded = &ender.degraded;
    loop {
        match armed {
            Ok(mut source) => {
                if degraded.replace(false) {
                    session.say(&format!("{SAID}git watch: armed again"));
                    act(&guard.rescan(ender), guard, ender);
                }
                loop {
                    match source.next() {
                        Next::Changed(changes) => {
                            ender.overflows.set(0);
                            act(&guard.changed(&changes, ender), guard, ender);
                        }
                        Next::Overflow => overflowed(guard, ender),
                        Next::Broken(error) => {
                            degraded.set(true);
                            session.say(&format!(
                                "{SAID}git watch: the watch failed ({}); rescanning the folder \
                                 every {} s until it is armed again",
                                code(&error),
                                RESCAN_EVERY.as_secs()
                            ));
                            break;
                        }
                        Next::Stopped => return,
                    }
                }
            }
            Err(error) => {
                if !degraded.get() {
                    session.say(&format!(
                        "{SAID}git watch: can't watch the folder ({}); rescanning it every {} s \
                         instead",
                        code(&error),
                        RESCAN_EVERY.as_secs()
                    ));
                    degraded.set(true);
                }
            }
        }
        act(&guard.rescan(ender), guard, ender);
        if session.pause(RESCAN_EVERY) {
            return;
        }
        armed = arm();
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
    act(&guard.rescan(ender), guard, ender);
}

/// Tells the lawyer of each item `live` moved and counts what went before it could be moved;
/// when any couldn't be moved, ends the session (if the move's own kill didn't), then tries
/// those again.
fn act<T: Session>(live: &Live, guard: &impl Guard, ender: &Ender<'_, T>) {
    let session = ender.session;
    tell(live, session);
    count_gone(&live.gone, ender);
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
        let again = guard.retry(&left, ender);
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

/// Counts each path gone; one gone [`GONE_LIMIT`] times ends the session.
fn count_gone<T: Session>(gone: &[PathBuf], ender: &Ender<'_, T>) {
    let mut over = false;
    if let Ok(mut counts) = ender.gone.try_borrow_mut() {
        for path in gone {
            let count = counts.entry(path.clone()).or_insert(0);
            *count = count.saturating_add(1);
            over |= *count >= GONE_LIMIT;
        }
    }
    if over {
        ender.stop(Cause::Gone);
    }
}

/// Two lines for each item moved: the lawyer's notice, where it went, and the line the host
/// reads back as a [`Denial::SetAside`] ([`denial_from_line`]) for the session's denial sink,
/// which tells the agent.
fn tell(live: &Live, session: &impl Session) {
    for moved in &live.moved {
        tracing::info!(id = moved.entry.id, "the git watch set an item aside");
        let to = &moved.entry.to;
        session.say(&format!("{SAID}{}", moved.what.notice(&moved.path, to)));
        session.say(&format!(
            "{SAID}{SET_ASIDE}\t{}\t{}\t{}",
            moved.path.display(),
            to.display(),
            moved.what.why()
        ));
    }
}

/// The [`Denial::SetAside`] a line of the outer's stderr reports, if it is one the watch said.
/// The Windows launcher passes each line to it and each denial to the launch's sink: the runner
/// is another process, and its stderr is the one way back.
pub fn denial_from_line(line: &str) -> Option<Denial> {
    let mut fields = line.strip_prefix(SAID)?.split('\t');
    if fields.next()? != SET_ASIDE {
        return None;
    }
    let (path, to, why) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    let what = SetAsideWhat::ALL
        .into_iter()
        .find(|what| what.why() == why)?;
    Some(Denial::SetAside {
        path: PathBuf::from(path),
        to: PathBuf::from(to),
        what,
    })
}

/// An error's code, or its kind: never a path.
fn code(error: &io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |code| code.to_string())
}
