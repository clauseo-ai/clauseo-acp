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
