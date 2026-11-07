//! The OS asked about every target at once, within one budget ([`ask_all`]), and the lookups
//! still running past it ([`Late`]), whose answers make the route again once they land. Each
//! answer is a proxy, an explicit `DIRECT`, or unknown ([`OsAnswer`]): a failed, late or
//! slot-refused lookup is no answer, so a refresh keeps the earlier one for up to
//! [`KEEP_FOR`] ([`Known::merge`]), unless it is a proxy that no longer takes a connection
//! ([`Reach`], probed within the budget).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use url::Url;

use super::OsLookup;

/// An OS proxy lookup's answer for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OsAnswer {
    /// Through this proxy.
    Proxy(String),
    /// The OS said `DIRECT`, or sets no proxy.
    Direct,
    /// No answer: the lookup failed or timed out, found no free slot, or ran past the budget.
    Unknown,
}

impl From<Option<String>> for OsAnswer {
    /// A proxy, else `DIRECT` (the tests' lookups).
    fn from(proxy: Option<String>) -> Self {
        proxy.map_or(OsAnswer::Direct, OsAnswer::Proxy)
    }
}

impl OsAnswer {
    /// The proxy to use: none for `DIRECT` and for no answer.
    pub(crate) fn proxy(&self) -> Option<String> {
        match self {
            OsAnswer::Proxy(proxy) => Some(proxy.clone()),
            OsAnswer::Direct | OsAnswer::Unknown => None,
        }
    }
}

/// Each target's answer.
pub(crate) type Answered = BTreeMap<Url, OsAnswer>;

/// How long an answer a real lookup gave is kept over later lookups with no answer, from when
/// a real lookup last gave it: past it, no answer goes direct as in a first route, so a laptop
/// whose new network fails every lookup does not keep an office's proxy for ever.
pub(crate) const KEEP_FOR: Duration = Duration::from_mins(10);

/// The longest one proxy's probe takes ([`Reach`]): its host resolved and a TCP connect to its
/// port. A resolve starts the probes it needs at the latest this long before its budget ends,
/// so they end within it.
pub(crate) const PROBE_CAP: Duration = Duration::from_secs(1);

/// Whether a proxy (`http://host:port`) takes a TCP connection within [`PROBE_CAP`]: connect
/// and close, nothing sent. Blocking.
pub(crate) type Reach = dyn Fn(&str) -> bool + Send + Sync;

/// The real [`Reach`]: `clauseo_sandbox::proxy::upstream::reachable`. A proxy that is not an
/// `http://host:port` URL (never: [`OsAnswer::Proxy`] holds one) is not judged, so it counts
/// as reachable and is kept as before.
pub(crate) fn reach(proxy: &str) -> bool {
    let Ok(url) = Url::parse(proxy) else {
        return true;
    };
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
        return true;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    clauseo_sandbox::proxy::upstream::reachable(host, port, PROBE_CAP)
}

/// The probes a resolve ran: whether each proxy answered. A proxy not in it (not probed, or its
/// probe did not end in time) is not known to be dead.
pub(crate) type Probed = BTreeMap<String, bool>;

/// Whether `proxy`'s probe found it dead.
fn dead(probed: &Probed, proxy: &str) -> bool {
    probed.get(proxy) == Some(&false)
}

/// The answers behind a route, with when a real lookup last gave each (a monotonic reading).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Known {
    pub(crate) answers: Answered,
    confirmed: BTreeMap<Url, Duration>,
    /// The targets whose proxy was kept over one `DIRECT`, the proxy still reachable: the next
    /// `DIRECT` for them is taken.
    direct_once: BTreeSet<Url>,
}

impl Known {
    /// Each target's proxy that a resolve asking about `targets` may keep over a fresh answer
    /// ([`Known::merge`]), so the proxies it may probe: none when `targets` are other hosts.
    pub(crate) fn keepable(&self, targets: &BTreeSet<Url>) -> BTreeMap<Url, String> {
        if !self.answers.keys().eq(targets.iter()) {
            return BTreeMap::new();
        }
        self.answers
            .iter()
            .filter_map(|(target, answer)| Some((target.clone(), answer.proxy()?)))
            .collect()
    }

    /// `fresh`, got at `now`, over these, with the proxies `probed` in this resolve. When
    /// `fresh` asks about other targets (a first route, or settings that change them) it is
    /// taken as it is, no answer going direct. When it asks about the same ones (a refresh, or
    /// settings that leave them):
    /// - a proxy is taken, confirmed now;
    /// - no answer keeps this route's answer for its target when that was confirmed under
    ///   `keep_for` ago and is not a proxy the probe found dead: a proxy survives a transient
    ///   failure, but not a network where it cannot be reached (the VPN dropped, a PAC on an
    ///   internal server failing with it); else it stays unknown, so that target goes direct;
    /// - `DIRECT` over a proxy the probe did not find dead keeps the proxy this once (a WPAD that
    ///   failed once and read as "no proxy here"); the next `DIRECT` for it, or one over a dead
    ///   proxy, is taken. Any other `DIRECT` is taken, confirmed now.
    pub(crate) fn merge(
        &self,
        fresh: Answered,
        now: Duration,
        keep_for: Duration,
        probed: &Probed,
    ) -> Known {
        let same = self.answers.keys().eq(fresh.keys());
        let mut known = Known::default();
        for (target, answer) in fresh {
            let previous = same
                .then(|| self.answers.get(&target))
                .flatten()
                .zip(self.confirmed.get(&target).copied());
            let once = self.direct_once.contains(&target);
            let alive = |kept: &OsAnswer| !kept.proxy().is_some_and(|p| dead(probed, &p));
            match (answer, previous) {
                (OsAnswer::Unknown, Some((kept, at)))
                    if now.saturating_sub(at) < keep_for && alive(kept) =>
                {
                    known.keep(target, kept, at, once);
                }
                (OsAnswer::Direct, Some((kept, at)))
                    if !once && kept.proxy().is_some() && alive(kept) =>
                {
                    known.keep(target, kept, at, true);
                }
                (OsAnswer::Unknown, _) => {
                    known.answers.insert(target, OsAnswer::Unknown);
                }
                (answer, _) => known.take(target, answer, now),
            }
        }
        known
    }

    /// With the real answers in `late` (lookups past the budget that landed at `now`) taken,
    /// a `DIRECT` over a proxy as in [`Known::merge`], with the resolve's `probed`.
    pub(crate) fn landed(mut self, late: Answered, now: Duration, probed: &Probed) -> Known {
        for (target, answer) in late {
            let kept = self.answers.get(&target).and_then(OsAnswer::proxy);
            match answer {
                OsAnswer::Unknown => {}
                OsAnswer::Direct
                    if !self.direct_once.contains(&target)
                        && kept.is_some_and(|p| !dead(probed, &p)) =>
                {
                    self.direct_once.insert(target);
                }
                answer => self.take(target, answer, now),
            }
        }
        self
    }

    /// `answer` for `target`, kept from an earlier route with when it was confirmed.
    fn keep(&mut self, target: Url, answer: &OsAnswer, at: Duration, direct_once: bool) {
        if direct_once {
            self.direct_once.insert(target.clone());
        }
        self.confirmed.insert(target.clone(), at);
        self.answers.insert(target, answer.clone());
    }

    /// `answer` for `target`, a real lookup's, confirmed `now`.
    fn take(&mut self, target: Url, answer: OsAnswer, now: Duration) {
        self.direct_once.remove(&target);
        self.confirmed.insert(target.clone(), now);
        self.answers.insert(target, answer);
    }
}

/// The proxies a resolve may keep ([`Known::keepable`]), and how each is probed.
pub(crate) struct Probe {
    pub(crate) keepable: BTreeMap<Url, String>,
    pub(crate) reach: Arc<Reach>,
}

impl Probe {
    /// One that probes nothing (a first route; tests).
    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            keepable: BTreeMap::new(),
            reach: Arc::new(|_proxy: &str| true),
        }
    }
}

/// The probes [`ask_all`] started, each on a thread of its own, once per proxy.
struct Probing {
    probe: Probe,
    started: BTreeSet<String>,
    running: usize,
    send: mpsc::Sender<(String, bool)>,
    results: mpsc::Receiver<(String, bool)>,
}

impl Probing {
    fn new(probe: Probe) -> Self {
        let (send, results) = mpsc::channel();
        Self {
            probe,
            started: BTreeSet::new(),
            running: 0,
            send,
            results,
        }
    }

    /// Probes `target`'s keepable proxy, unless it has none or its proxy is probed already.
    fn start(&mut self, target: &Url) {
        let Some(proxy) = self.probe.keepable.get(target) else {
            return;
        };
        if !self.started.insert(proxy.clone()) {
            return;
        }
        let (reach, send, proxy) = (
            Arc::clone(&self.probe.reach),
            self.send.clone(),
            proxy.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("clauseo-proxy-reach".to_owned())
            .spawn(move || {
                let reached = reach(&proxy);
                // The resolve may be over: then no one reads it.
                let _sent = send.send((proxy, reached));
            });
        match spawned {
            Ok(_thread) => self.running += 1,
            Err(err) => tracing::warn!(%err, "a proxy probe did not start; it is not known dead"),
        }
    }

    /// The probes that ended by `deadline`.
    fn finish(self, clock: SystemClock, deadline: Duration) -> Probed {
        let mut probed = Probed::new();
        while probed.len() < self.running {
            let left = deadline.saturating_sub(clock.monotonic());
            let Ok((proxy, reached)) = self.results.recv_timeout(left) else {
                break;
            };
            probed.insert(proxy, reached);
        }
        probed
    }
}

/// `os`'s answer for each of `targets`, all asked at once, each on its own thread: what has
/// not answered within `budget` is [`OsAnswer::Unknown`] in this route. A lookup still running
/// is left to end on its thread (the sandbox bounds each one); its answer comes in
/// [`Answers::late`].
///
/// A keepable proxy ([`Probe`]) is probed once a fresh answer could replace it: when its
/// target's lookup answers `DIRECT` or nothing, or is still running [`PROBE_CAP`] before the
/// budget ends. So every probe ends within the budget, and none runs while the OS keeps
/// answering with proxies.
pub(crate) fn ask_all(
    targets: BTreeSet<Url>,
    os: &Arc<OsLookup>,
    budget: Duration,
    probe: Probe,
) -> Answers {
    let clock = SystemClock;
    let deadline = clock.monotonic() + budget;
    let probe_from = deadline.saturating_sub(PROBE_CAP);
    let mut probing = Probing::new(probe);
    let (send, answers) = mpsc::channel();
    let mut waiting = BTreeSet::new();
    let mut got: Answered = targets
        .iter()
        .map(|target| (target.clone(), OsAnswer::Unknown))
        .collect();
    for target in targets {
        let (os, send, asked) = (Arc::clone(os), send.clone(), target.clone());
        let spawned = std::thread::Builder::new()
            .name("clauseo-proxy-lookup".to_owned())
            .spawn(move || {
                let answer = os(&asked);
                // The route may be made already: then no one reads it.
                let _sent = send.send((asked, answer));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "an OS proxy lookup did not start; it has no answer");
        } else {
            waiting.insert(target);
        }
    }
    let mut probing_waiting = false;
    while !waiting.is_empty() {
        let now = clock.monotonic();
        if !probing_waiting && now >= probe_from {
            probing_waiting = true;
            for target in &waiting {
                probing.start(target);
            }
        }
        let until = if probing_waiting {
            deadline
        } else {
            probe_from
        };
        match answers.recv_timeout(until.saturating_sub(now)) {
            Ok((target, answer)) => {
                if answer.proxy().is_none() {
                    probing.start(&target);
                }
                waiting.remove(&target);
                got.insert(target, answer);
            }
            Err(_) if !probing_waiting => {}
            Err(_) => break,
        }
    }
    let late = (!waiting.is_empty()).then(|| {
        tracing::warn!(
            late = waiting.len(),
            "OS proxy lookups past the budget have no answer in this route"
        );
        Late {
            running: waiting.len(),
            answers,
        }
    });
    let probed = probing.finish(clock, deadline);
    Answers { got, late, probed }
}

/// The OS's answers [`ask_all`] got within its budget, and the lookups still running past it.
pub(crate) struct Answers {
    /// Each target's answer, [`OsAnswer::Unknown`] for one not in time.
    pub(crate) got: Answered,
    /// The lookups past the budget, when any.
    pub(crate) late: Option<Late>,
    /// The keepable proxies probed within the budget.
    pub(crate) probed: Probed,
}

/// Lookups still running past the budget: each one's answer comes on `answers` as it ends.
pub(crate) struct Late {
    pub(crate) running: usize,
    pub(crate) answers: mpsc::Receiver<(Url, OsAnswer)>,
}

impl Late {
    /// The late answers that end within `wait` (the sandbox bounds each lookup at 10 s).
    pub(crate) fn collect(self, wait: Duration) -> Answered {
        let clock = SystemClock;
        let deadline = clock.monotonic() + wait;
        let mut got = BTreeMap::new();
        while got.len() < self.running {
            let left = deadline.saturating_sub(clock.monotonic());
            let Ok((target, answer)) = self.answers.recv_timeout(left) else {
                break;
            };
            got.insert(target, answer);
        }
        got
    }
}
