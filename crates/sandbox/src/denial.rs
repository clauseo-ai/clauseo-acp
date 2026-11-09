//! The denial types every stage C task shares, the sink a launch reports them to, and the rate
//! cap in front of that sink.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Something the jail or the egress proxy refused. One definition for every OS and for the
/// proxy; `host` turns each into a denial event and, where it applies, a card (R18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Denial {
    /// A file access the jail refused.
    Path {
        /// The path the agent asked for, as the OS reported it.
        path: PathBuf,
        /// Read or write.
        access: Access,
        /// Why the policy holds it out, from [`Policy::classify`](crate::Policy::classify).
        class: PathClass,
    },
    /// A connection the egress proxy refused.
    Host {
        /// The host name (or address) the agent asked for.
        host: String,
        /// The port it asked for.
        port: u16,
        /// Why it was refused.
        why: HostRefusal,
        /// The host serves many unrelated sites, so allowing it allows them all.
        shared_hosting: bool,
    },
    /// The upstream proxy asked for credentials Clauseo does not have (a 407).
    UpstreamAuth {
        /// The upstream proxy, as configured.
        upstream: String,
    },
    /// This many denials were held back by the rate cap ([`CappedSink`]) in the last window.
    Dropped {
        /// How many.
        count: u32,
    },
    /// The egress proxy refused a connection because the firm's proxy lookup failed: the
    /// settings name a PAC URL or a static proxy, or the PAC script would not run, and no route
    /// for the host from the last 10 minutes was left to reuse (decision record 0039). Going
    /// direct would pass around the firm's proxy.
    UpstreamUnavailable {
        /// The host name (or address) the agent asked for.
        host: String,
        /// The port it asked for.
        port: u16,
    },
    /// A connection the egress proxy refused because the host is on the block list (ADR 0053).
    /// No allowlist entry lets one through.
    Blocked {
        /// The host the agent asked for, as the proxy reads it (lowercase, ASCII, no trailing
        /// dot; an address without brackets).
        host: String,
        /// The port it asked for.
        port: u16,
        /// The list entry that matched: the host or a parent domain, or `[address]`.
        listed: String,
        /// Why the entry is listed.
        tag: clauseo_blocklist::Tag,
        /// When the list that matched was made.
        generated: clauseo_blocklist::Generated,
    },
    /// The macOS git watch found something in the matter while a session ran that the
    /// lawyer's own git tools, or an editor's, would trust: a `.git`, a bare repository's
    /// folder or `HEAD`, a part of a launch repository's common dir, or a link leading out of
    /// the matter (decision record 0038). git would run its hooks and `core.fsmonitor` as the
    /// lawyer. The watch ended the session first, then set the item aside.
    /// [`Denial::notice`] is the lawyer's text. Never rate-capped.
    GitSetAside {
        /// Its path, relative to the matter's root.
        path: PathBuf,
        /// What it was.
        what: GitItem,
        /// What became of it.
        outcome: GitOutcome,
        /// The watch ended the session for it; false for one a session left whose watch never
        /// ran its final scan (the host quit), found at the matter's next launch.
        session_ended: bool,
    },
    /// The macOS git watch lost its view of the matter: its `FSEvents` stream stopped
    /// delivering (its canary's event did not come within 2 s), or its canary could not be
    /// written (decision record 0038). It ended the session first, then ran its final scan.
    /// [`Denial::notice`] is the lawyer's text. Never rate-capped.
    GitWatchLost,
}

/// What the git watch set aside ([`Denial::GitSetAside`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitItem {
    /// A `.git` folder, or a `.git` file naming a `gitdir:`.
    DotGit,
    /// A new folder laid out as a bare repository: `HEAD`, `objects` and `refs`.
    BareRepo,
    /// A `HEAD` file or link in a folder that was not a repository at launch.
    BareRepoHead,
    /// A link whose target is outside the matter, or can't be found.
    OutsideLink,
    /// A new `objects`, `refs`, `config` (or another name git reads there) at a launch
    /// repository's `commondir` target.
    CommonDirPart,
}

/// What became of an item the git watch set aside ([`Denial::GitSetAside`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOutcome {
    /// Moved out of the matter, to this path in Clauseo's set-aside folder.
    Moved(PathBuf),
    /// It could not move, so it was renamed where it is, to this path, with any `HEAD` and
    /// `config` inside renamed too: git no longer reads it.
    RenamedInPlace(PathBuf),
    /// A link that could not move was removed (a link holds no data); it led here.
    LinkRemoved {
        /// Where it led, as it was written.
        target: PathBuf,
    },
    /// Neither: Clauseo refuses the matter's next launch until it is gone.
    Stuck,
}

impl Denial {
    /// The agent's refusal line, for the denials that carry one: [`Denial::GitSetAside`].
    /// Its path is shown with [`shown`].
    pub fn refusal(&self) -> Option<String> {
        let Denial::GitSetAside { path, what, .. } = self else {
            return None;
        };
        let path = shown(path);
        Some(match what {
            GitItem::OutsideLink => format!(
                "Clauseo ended this session and set aside {path}: a link inside a matter can't \
                 lead outside it."
            ),
            GitItem::DotGit
            | GitItem::BareRepo
            | GitItem::BareRepoHead
            | GitItem::CommonDirPart => format!(
                "Clauseo ended this session and set aside {path}: a matter can't hold a git \
                 repository while a session runs."
            ),
        })
    }

    /// The lawyer's notice, for the denials that carry one: [`Denial::GitSetAside`] and
    /// [`Denial::GitWatchLost`]. The agent runs as the lawyer's own user, so no one can tell
    /// who made the item: the notice says what appeared and what Clauseo did, and blames no
    /// one. Every path is shown with [`shown`].
    pub fn notice(&self) -> Option<String> {
        if *self == Denial::GitWatchLost {
            return Some(
                "Clauseo stopped the AI tool because it could no longer watch this matter's \
                 folder for changes. You can start it again."
                    .to_owned(),
            );
        }
        let Denial::GitSetAside {
            path,
            what,
            outcome,
            session_ended,
        } = self
        else {
            return None;
        };
        let at = shown(path);
        let item = match what {
            GitItem::DotGit | GitItem::BareRepo => "A Git folder",
            GitItem::BareRepoHead => "A Git HEAD file",
            GitItem::CommonDirPart => "A part of a Git folder",
            GitItem::OutsideLink => "A link leading outside the matter",
        };
        let when = if *session_ended {
            "while an AI tool was running"
        } else {
            "during an AI session Clauseo could not finish checking"
        };
        let why = match what {
            GitItem::OutsideLink => {
                "because Git folders can run programs when opened, and a link can lead to one"
            }
            GitItem::DotGit
            | GitItem::BareRepo
            | GitItem::BareRepoHead
            | GitItem::CommonDirPart => "because Git folders can run programs when opened",
        };
        let stopped = if *session_ended {
            "Clauseo stopped the session and "
        } else {
            "Clauseo "
        };
        let then = match outcome {
            GitOutcome::Moved(to) => format!(
                "{stopped}moved it to {}, {why}. If you put it there yourself, you can move it \
                 back from there.",
                shown(to)
            ),
            GitOutcome::RenamedInPlace(to) => format!(
                "{stopped}renamed it to {} where it is, as it could not be moved off the \
                 matter's disk, {why}.",
                shown(to)
            ),
            GitOutcome::LinkRemoved { target } => format!(
                "{stopped}removed the link, which led to {}, {why}.",
                shown(target)
            ),
            GitOutcome::Stuck if *session_ended => format!(
                "Clauseo stopped the session, {why}, but could not move or rename it. Clauseo \
                 won't start an AI tool in this matter until {at} is moved out of it."
            ),
            GitOutcome::Stuck => format!(
                "Clauseo could not move or rename it, and won't start an AI tool in this \
                 matter until {at} is moved out of it."
            ),
        };
        Some(format!(
            "{item} appeared in the matter at {at} {when}. {then}"
        ))
    }
}

/// `path` as a notice or refusal shows it, so two different names never look the same and a
/// crafted one can neither break the line, hide a letter nor turn the text around: a
/// backslash is doubled; each control character, bidirectional control (U+061C, U+200E,
/// U+200F, U+202A to U+202E, U+2066 to U+2069), zero-width or invisible character (U+200B to
/// U+200D, U+2060 to U+2064, U+FEFF) and line or paragraph separator is shown as its escape
/// (`\u{202e}`); and each byte that isn't UTF-8 as `\x80`.
pub(crate) fn shown(path: &std::path::Path) -> String {
    let mut out = String::new();
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        for c in chunk.valid().chars() {
            let hidden = matches!(
                c,
                '\u{061C}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{2028}'..='\u{202E}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{FEFF}'
            );
            if c == '\\' {
                out.push_str("\\\\");
            } else if c.is_control() || hidden {
                out.extend(c.escape_unicode());
            } else {
                out.push(c);
            }
        }
        for byte in chunk.invalid() {
            out.push_str("\\x");
            out.push(hex(byte >> 4));
            out.push(hex(byte & 0xf));
        }
    }
    out
}

/// The lowercase hex digit of `n`, below 16.
fn hex(n: u8) -> char {
    char::from_digit(u32::from(n), 16).unwrap_or('?')
}

/// The kind of file access refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Reading the file or listing the folder.
    Read,
    /// Creating, changing, renaming or deleting it.
    Write,
}

/// Why a refused path is out of the agent's reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    /// Outside everything the policy allows: the lawyer may widen it with a card.
    Outside,
    /// Under a hidden path (the data dir, another matter, a refused folder): never widened.
    Hidden,
    /// A write to a path the policy allows to read only (`.git`, a protected name, an over-cap
    /// file, an allowed folder, the runtime).
    ReadOnly,
    /// A link in the matter that leads outside it, or what it leads to: no card (S14).
    LinkTarget,
}

/// Why the egress proxy refused a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRefusal {
    /// The host is not on the matter's allowlist.
    NotAllowed,
    /// The host resolved to a private, loopback or link-local address.
    PrivateAddress,
    /// The port is not one the proxy forwards.
    Port,
    /// The request was not a CONNECT (or a scheme the proxy does not carry).
    Scheme,
}

/// Whether the egress proxy can reach the internet at all, reported when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reachability {
    /// The upstream (direct or the OS's proxy) answers.
    Reachable,
    /// It does not: every connection fails, so the denials are not the allowlist's doing.
    Unreachable,
}

/// Where one launch's denials go. One sink per launch; the jail's watcher and the proxy both
/// call it, from any thread. It gets every denial, with no dedupe (R18 dedupes per turn); put a
/// [`CappedSink`] in front of it for the rate cap.
///
/// Implementations must not block. The proxy's and the watcher's threads call it, so a sink
/// that blocks stalls them, and can deadlock `ProxyHandle`'s drop. Post the denial somewhere
/// (a channel, a queue) and return.
pub trait DenialSink: Send + Sync {
    /// One denial.
    fn denied(&self, d: Denial);
    /// The proxy's upstream became reachable or unreachable. Ignored unless overridden.
    fn upstream(&self, _r: Reachability) {}
}

impl<T: DenialSink + ?Sized> DenialSink for Arc<T> {
    fn denied(&self, d: Denial) {
        (**self).denied(d);
    }

    fn upstream(&self, r: Reachability) {
        (**self).upstream(r);
    }
}

/// At most this many denials per window reach a [`CappedSink`]'s inner sink.
pub const DENIALS_PER_WINDOW: u32 = 20;

/// The rate cap's window.
pub const DENIAL_WINDOW: Duration = Duration::from_secs(1);

/// Time for the rate cap, as the time since some fixed start. An input, so tests need no sleeps.
pub trait Clock: Send + Sync {
    /// The time now, since the clock's own start.
    fn now(&self) -> Duration;
}

/// The real monotonic clock, counted from its construction.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// A clock that starts now.
    #[expect(
        clippy::disallowed_methods,
        reason = "the one real clock of the rate cap; tests pass their own Clock"
    )]
    pub fn new() -> SystemClock {
        SystemClock {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> SystemClock {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }
}

/// The rate cap: a [`DenialSink`] that passes at most [`DENIALS_PER_WINDOW`] denials per
/// [`DENIAL_WINDOW`] to its inner sink. A window starts at the first denial after the last one
/// ended. The excess is counted and reaches the inner sink as one [`Denial::Dropped`] when the
/// next window starts (at the next denial, before it) or at the first [`CappedSink::tick`]
/// after the window ends, whichever is first; the owner calls `tick` from its own timer so a
/// burst's count is not held until the next denial. [`DenialSink::upstream`] passes through
/// uncapped. Safe from many threads: the count is behind a mutex, and the inner sink is
/// called after the mutex is released, so it may block or call back.
#[derive(Debug)]
pub struct CappedSink<S, C = SystemClock> {
    inner: S,
    clock: C,
    window: Mutex<Window>,
}

/// One window's counts.
#[derive(Debug, Default)]
struct Window {
    /// When it started; `None` before the first denial and after a `tick` ended it.
    start: Option<Duration>,
    /// Denials passed on in it.
    passed: u32,
    /// Denials held back in it.
    dropped: u32,
}

impl Window {
    /// Ends the window if it is over at `now`, returning its held-back count if any.
    fn end_if_over(&mut self, now: Duration) -> Option<u32> {
        let start = self.start?;
        if now.saturating_sub(start) < DENIAL_WINDOW {
            return None;
        }
        let dropped = std::mem::take(&mut self.dropped);
        *self = Window::default();
        (dropped > 0).then_some(dropped)
    }
}

impl<S: DenialSink> CappedSink<S, SystemClock> {
    /// A cap on the real clock.
    pub fn new(inner: S) -> CappedSink<S, SystemClock> {
        CappedSink::with_clock(inner, SystemClock::new())
    }
}

impl<S: DenialSink, C: Clock> CappedSink<S, C> {
    /// A cap on `clock`.
    pub fn with_clock(inner: S, clock: C) -> CappedSink<S, C> {
        CappedSink {
            inner,
            clock,
            window: Mutex::new(Window::default()),
        }
    }

    /// The sink the cap passes denials to.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Delivers the held-back count as one [`Denial::Dropped`] if the window has ended. Call it
    /// about once a second; it does nothing while the window runs or when nothing was held.
    pub fn tick(&self) {
        let late = self.lock().end_if_over(self.clock.now());
        if let Some(count) = late {
            self.inner.denied(Denial::Dropped { count });
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Window> {
        // The counts stay valid whatever a panicking thread left; keep counting.
        self.window.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<S: DenialSink, C: Clock> DenialSink for CappedSink<S, C> {
    fn denied(&self, d: Denial) {
        let now = self.clock.now();
        let (late, pass) = {
            let mut window = self.lock();
            let late = window.end_if_over(now);
            if window.start.is_none() {
                window.start = Some(now);
            }
            let pass = window.passed < DENIALS_PER_WINDOW;
            if pass {
                window.passed += 1;
            } else {
                window.dropped = window.dropped.saturating_add(1);
            }
            (late, pass)
        };
        if let Some(count) = late {
            self.inner.denied(Denial::Dropped { count });
        }
        if pass {
            self.inner.denied(d);
        }
    }

    fn upstream(&self, r: Reachability) {
        self.inner.upstream(r);
    }
}

#[cfg(test)]
mod tests;
