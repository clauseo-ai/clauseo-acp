//! The outer runner's live watch for a git repository made in the matter (S15 tier B). The agent
//! can rename a lawyer's own folder or file to `.git`, or fill one with a bare repository's
//! `HEAD`, `objects` and `refs`; it stays the lawyer's, so git's ownership
//! check trusts it, and the lawyer's VS Code polling (or the lawyer's own git) would run its
//! `core.fsmonitor` or hooks as the lawyer before the after-session set-aside moves it. The outer
//! runs as the lawyer for the whole session, so it watches the matter's folder itself, always on
//! and depending on nothing IT can switch off (no `SandboxWatch`, no ETW):
//!
//! - `ReadDirectoryChangesW` on the matter's folder (`os.rs`), recursive, for names made and
//!   renamed ([`added_names`]: `FILE_ACTION_ADDED` and `FILE_ACTION_RENAMED_NEW_NAME`), armed
//!   before the inner runner starts. Each name reported goes to the set-aside's live check
//!   (`set_aside/live.rs`), which moves an unknown `.git`, bare repository, `HEAD` or link out
//!   of the matter at once, by the after-session pass's own move, manifest and lock, and scans
//!   below a folder made or moved in.
//! - An overflow (`ERROR_NOTIFY_ENUM_DIR`, an empty or unreadable buffer) rescans the whole
//!   matter and reads on. A watch that breaks, or can't be armed, rescans every
//!   [`RESCAN_EVERY`] and tries to arm again, saying so once (degraded).
//! - An item that can't be moved (the agent holding a handle to it, say) ends the session first
//!   (the job ended and emptied, as every end path does), then is tried again: fail closed.
//! - Each item moved gets one plain line on the outer's stderr, its path and why
//!   ([`SetAsideWhat::words`]), which the host reads back as a [`Denial::SetAside`] for the
//!   agent; the manifest lists it, as it lists the after-session pass's. That pass stays the
//!   backstop.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::windows::set_aside::{Failed, Live};
use crate::{Denial, SetAsideWhat};

#[cfg(windows)]
pub(super) mod os;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
#[cfg(test)]
mod tests;

/// How often the matter is rescanned while the watch can't be armed, or after it broke.
pub(crate) const RESCAN_EVERY: Duration = Duration::from_secs(2);
/// How many more tries a `.git` the watch couldn't move gets once the session ended.
const RETRIES: u32 = 5;
/// The pause between two of those tries, while the ended agent's handles close.
const RETRY_PAUSE: Duration = Duration::from_millis(200);
/// `FILE_ACTION_ADDED`.
const ADDED: u32 = 1;
/// `FILE_ACTION_RENAMED_NEW_NAME`.
const RENAMED_NEW_NAME: u32 = 5;

/// What starts each line the outer says on its stderr.
const SAID: &str = "clauseo-sandbox-run: ";

/// What the watch saw next.
#[derive(Debug)]
pub(crate) enum Next {
    /// Names made or renamed, relative to the matter's folder.
    Changed(Vec<PathBuf>),
    /// More changed than the watch could hold: what changed is unknown.
    Overflow,
    /// The watch failed and must be armed again.
    Broken(io::Error),
    /// The session ended: the watch is to stop.
    Stopped,
}

/// A watch armed on the matter's folder.
pub(crate) trait Source {
    /// Blocks until something changed or the watch is stopped.
    fn next(&mut self) -> Next;
}

/// The set-aside's live check (`set_aside::GitGuard` on Windows).
pub(crate) trait Guard {
    /// Checks the names the watch reported.
    fn changed(&self, paths: &[PathBuf]) -> Live;
    /// Scans the whole matter for an unknown `.git`.
    fn rescan(&self) -> Live;
    /// Tries again what a check couldn't move.
    fn retry(&self, failed: &[Failed]) -> Live;
}

/// What the watch asks of the session.
pub(crate) trait Session {
    /// Ends the session: the agent's job ended and emptied, by the outer's own end path.
    fn end(&self);
    /// One line on the outer's stderr.
    fn say(&self, line: &str);
    /// Waits up to `wait`; `true`, early, once the watch is stopped.
    fn pause(&self, wait: Duration) -> bool;
    /// Waits `wait`, stopped or not: between the tries after the session ended.
    fn settle(&self, wait: Duration);
}

/// Runs the watch until it is stopped: `first` is the watch armed before the agent started,
/// `arm` arms it again.
pub(crate) fn watch<S: Source>(
    first: io::Result<S>,
    mut arm: impl FnMut() -> io::Result<S>,
    guard: &impl Guard,
    session: &impl Session,
) {
    let mut armed = first;
    let mut degraded = false;
    loop {
        match armed {
            Ok(mut source) => {
                if std::mem::take(&mut degraded) {
                    session.say("clauseo-sandbox-run: git watch: armed again");
                    act(&guard.rescan(), guard, session);
                }
                loop {
                    match source.next() {
                        Next::Changed(paths) => act(&guard.changed(&paths), guard, session),
                        Next::Overflow => {
                            session.say(
                                "clauseo-sandbox-run: git watch: too many changes at once; \
                                 rescanning the matter",
                            );
                            act(&guard.rescan(), guard, session);
                        }
                        Next::Broken(error) => {
                            degraded = true;
                            session.say(&format!(
                                "clauseo-sandbox-run: git watch: the watch failed ({}); \
                                 rescanning the matter every {} s until it is armed again",
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
                if !degraded {
                    session.say(&format!(
                        "clauseo-sandbox-run: git watch: can't watch the matter ({}); \
                         rescanning it every {} s instead",
                        code(&error),
                        RESCAN_EVERY.as_secs()
                    ));
                    degraded = true;
                }
            }
        }
        act(&guard.rescan(), guard, session);
        if session.pause(RESCAN_EVERY) {
            return;
        }
        armed = arm();
    }
}

/// Tells the lawyer of each item `live` moved; when any couldn't be moved, ends the session,
/// then tries those again.
fn act(live: &Live, guard: &impl Guard, session: &impl Session) {
    tell(live, session);
    if live.failed.is_empty() {
        return;
    }
    for failed in &live.failed {
        session.say(&format!(
            "clauseo-sandbox-run: git watch: an item the agent made couldn't be set aside \
             ({}); ending the session first",
            failed.failure
        ));
    }
    session.end();
    let mut left = live.failed.clone();
    for _ in 0..RETRIES {
        let again = guard.retry(&left);
        tell(&again, session);
        left = again.failed;
        if left.is_empty() {
            return;
        }
        session.settle(RETRY_PAUSE);
    }
    session.say(&format!(
        "clauseo-sandbox-run: git watch: {} item(s) the agent made still couldn't be set aside \
         once the session ended; the check after the session tries again",
        left.len()
    ));
}

/// One line for each item moved, its path below the matter and why: the lawyer sees it in the
/// run's stderr, and the host reads it back as a [`Denial::SetAside`] ([`denial_from_line`])
/// for the session's denial sink, which tells the agent.
fn tell(live: &Live, session: &impl Session) {
    for moved in &live.moved {
        tracing::info!(id = moved.entry.id, "the git watch set an item aside");
        session.say(&format!("{SAID}{}", moved.what.words(&moved.path)));
    }
}

/// The [`Denial::SetAside`] a line of the outer's stderr reports, if it is one the watch said.
/// The Windows launcher passes each line to it and each denial to the launch's sink: the runner
/// is another process, and its stderr is the one way back.
pub fn denial_from_line(line: &str) -> Option<Denial> {
    let rest = line
        .strip_prefix(SAID)?
        .strip_prefix("Clauseo set aside ")?;
    let (path, why) = rest.split_once(": ")?;
    let what = SetAsideWhat::ALL
        .into_iter()
        .find(|what| what.why() == why)?;
    Some(Denial::SetAside {
        path: PathBuf::from(path),
        what,
    })
}

/// An error's code, or its kind: never a path.
fn code(error: &io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |code| code.to_string())
}

/// The names `FILE_ACTION_ADDED` and `FILE_ACTION_RENAMED_NEW_NAME` records in `buffer` give
/// (`FILE_NOTIFY_INFORMATION`, chained by `NextEntryOffset`), relative to the watched folder.
/// `None` when the buffer doesn't read as records: the watch then rescans.
pub(crate) fn added_names(buffer: &[u8]) -> Option<Vec<PathBuf>> {
    let mut names = Vec::new();
    let mut at = 0_usize;
    loop {
        let record = buffer.get(at..)?;
        let next = word(record, 0)?;
        let action = word(record, 4)?;
        let length = usize::try_from(word(record, 8)?).ok()?;
        let name = record.get(12..12_usize.checked_add(length)?)?;
        if action == ADDED || action == RENAMED_NEW_NAME {
            names.push(path_of(name)?);
        }
        if next == 0 {
            return Some(names);
        }
        at = at.checked_add(usize::try_from(next).ok()?)?;
    }
}

/// The little-endian `u32` at `at` in `bytes`.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    let four: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(four))
}

/// A name in a record: UTF-16LE, kept as it is on Windows, text or not.
fn path_of(name: &[u8]) -> Option<PathBuf> {
    let (pairs, odd) = name.as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    let units: Vec<u16> = pairs.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt as _;
        Some(PathBuf::from(std::ffi::OsString::from_wide(&units)))
    }
    #[cfg(not(windows))]
    {
        String::from_utf16(&units).ok().map(PathBuf::from)
    }
}
