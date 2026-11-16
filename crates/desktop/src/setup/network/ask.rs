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
/// port. A resolve starts the probes it needs at the latest this long, plus [`PROBE_MARGIN`],
/// before its budget ends, so they end within it.
pub(crate) const PROBE_CAP: Duration = Duration::from_secs(1);

/// The room left after a probe that takes its whole [`PROBE_CAP`] (a proxy that drops packets
/// silently) before the budget ends: its thread's start and hand-back, so a dead proxy is known
/// dead in this route rather than kept for one more refresh.
const PROBE_MARGIN: Duration = Duration::from_millis(200);

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

/// What [`ask_all`] waits for: a lookup's answer, or a probe's result with the reading of
/// [`LookupTime::now`] when it ended.
pub(crate) enum Event {
    Answer(Url, OsAnswer),
    Probed {
        proxy: String,
        reached: bool,
        ended: Duration,
    },
}

/// The time [`ask_all`] keeps: [`RealTime`], or a virtual one in tests, so a test decides when
/// its budget and a probe's cap run out rather than racing a loaded machine.
pub(crate) trait LookupTime: Send + Sync {
    /// A monotonic reading.
    fn now(&self) -> Duration;
    /// The next event on `events`, waiting at most until `until` (a reading of [`Self::now`]);
    /// `None` once it has passed. `probing` probes have started and not yet reported.
    fn next(
        &self,
        events: &mpsc::Receiver<Event>,
        until: Duration,
        probing: usize,
    ) -> Option<Event>;
}

/// The real time: `SystemClock`'s monotonic reading, and a wait of that long.
pub(crate) struct RealTime;

impl RealTime {
    /// One to share.
    pub(crate) fn shared() -> Arc<dyn LookupTime> {
        Arc::new(RealTime)
    }
}

impl LookupTime for RealTime {
    fn now(&self) -> Duration {
        SystemClock.monotonic()
    }

    fn next(
        &self,
        events: &mpsc::Receiver<Event>,
        until: Duration,
        _probing: usize,
    ) -> Option<Event> {
        events.recv_timeout(until.saturating_sub(self.now())).ok()
    }
}

/// The probes [`ask_all`] started, each on a thread of its own, once per proxy, and the results
/// of those that ended before the deadline.
struct Probing {
    probe: Probe,
    started: BTreeSet<String>,
    /// Started and not yet reported.
    running: usize,
    probed: Probed,
}

impl Probing {
    fn new(probe: Probe) -> Self {
        Self {
            probe,
            started: BTreeSet::new(),
            running: 0,
            probed: Probed::new(),
        }
    }

    /// Probes `target`'s keepable proxy, unless it has none or its proxy is probed already; the
    /// result comes on `send`, stamped with `time`.
    fn start(&mut self, target: &Url, send: &mpsc::Sender<Event>, time: &Arc<dyn LookupTime>) {
        let Some(proxy) = self.probe.keepable.get(target) else {
            return;
        };
        if !self.started.insert(proxy.clone()) {
            return;
        }
        let (reach, send, time, proxy) = (
            Arc::clone(&self.probe.reach),
            send.clone(),
            Arc::clone(time),
            proxy.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("clauseo-proxy-reach".to_owned())
            .spawn(move || {
                let reached = reach(&proxy);
                let ended = time.now();
                // The resolve may be over: then no one reads it.
                let _sent = send.send(Event::Probed {
                    proxy,
                    reached,
                    ended,
                });
            });
        match spawned {
            Ok(_thread) => self.running += 1,
            Err(err) => tracing::warn!(%err, "a proxy probe did not start; it is not known dead"),
        }
    }

    /// A probe's result: it counts only when it ended before `deadline`.
    fn landed(&mut self, proxy: String, reached: bool, ended: Duration, deadline: Duration) {
        self.running = self.running.saturating_sub(1);
        if ended < deadline {
            self.probed.insert(proxy, reached);
        }
    }
}

/// `os`'s answer for each of `targets`, all asked at once, each on its own thread: what has
/// not answered within `budget` is [`OsAnswer::Unknown`] in this route. A lookup still running
/// is left to end on its thread (the sandbox bounds each one); its answer comes in
/// [`Answers::late`].
///
/// A keepable proxy ([`Probe`]) is probed once a fresh answer could replace it: when its
/// target's lookup answers `DIRECT` or nothing, or is still running [`PROBE_CAP`] and
/// [`PROBE_MARGIN`] before the budget ends. So every probe ends within the budget, and none runs
/// while the OS keeps answering with proxies; one that ends past it does not count. `time` is
/// [`RealTime`] but in tests.
pub(crate) fn ask_all(
    targets: BTreeSet<Url>,
    os: &Arc<OsLookup>,
    budget: Duration,
    probe: Probe,
    time: &Arc<dyn LookupTime>,
) -> Answers {
    let deadline = time.now() + budget;
    let probe_from = deadline.saturating_sub(PROBE_CAP + PROBE_MARGIN);
    let mut probing = Probing::new(probe);
    let (send, events) = mpsc::channel();
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
                let _sent = send.send(Event::Answer(asked, answer));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "an OS proxy lookup did not start; it has no answer");
        } else {
            waiting.insert(target);
        }
    }
    let mut probing_waiting = false;
    while !waiting.is_empty() {
        if !probing_waiting && time.now() >= probe_from {
            probing_waiting = true;
            for target in &waiting {
                probing.start(target, &send, time);
            }
        }
        let until = if probing_waiting {
            deadline
        } else {
            probe_from
        };
        match time.next(&events, until, probing.running) {
            Some(Event::Answer(target, answer)) => {
                if answer.proxy().is_none() {
                    probing.start(&target, &send, time);
                }
                waiting.remove(&target);
                got.insert(target, answer);
            }
            Some(Event::Probed {
                proxy,
                reached,
                ended,
            }) => probing.landed(proxy, reached, ended, deadline),
            None if !probing_waiting => {}
            None => break,
        }
    }
    // The probes still running, until the deadline; a lookup answering meanwhile is late.
    let mut landed_late = Answered::new();
    while probing.running > 0 {
        match time.next(&events, deadline, probing.running) {
            Some(Event::Probed {
                proxy,
                reached,
                ended,
            }) => probing.landed(proxy, reached, ended, deadline),
            Some(Event::Answer(target, answer)) => {
                landed_late.insert(target, answer);
            }
            None => break,
        }
    }
    let late = (!waiting.is_empty()).then(|| {
        tracing::warn!(
            late = waiting.len(),
            "OS proxy lookups past the budget have no answer in this route"
        );
        Late {
            running: waiting.len(),
            got: landed_late,
            events,
        }
    });
    Answers {
        got,
        late,
        probed: probing.probed,
    }
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

/// Lookups still running past the budget: each one's answer comes on `events` as it ends
/// (those in `got` came while the last probes were waited for).
pub(crate) struct Late {
    running: usize,
    got: Answered,
    events: mpsc::Receiver<Event>,
}

impl Late {
    /// The late answers that end within `wait` (the sandbox bounds each lookup at 10 s).
    pub(crate) fn collect(self, wait: Duration) -> Answered {
        let clock = SystemClock;
        let deadline = clock.monotonic() + wait;
        let mut got = self.got;
        while got.len() < self.running {
            let left = deadline.saturating_sub(clock.monotonic());
            match self.events.recv_timeout(left) {
                Ok(Event::Answer(target, answer)) => {
                    got.insert(target, answer);
                }
                Ok(Event::Probed { .. }) => {}
                Err(_) => break,
            }
        }
        got
    }
}
