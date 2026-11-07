//! The OS asked about every target at once, within one budget ([`ask_all`]), and the lookups
//! still running past it ([`Late`]), whose answers make the route again once they land. Each
//! answer is a proxy, an explicit `DIRECT`, or unknown ([`OsAnswer`]): a failed, late or
//! slot-refused lookup is no answer, so a refresh keeps the earlier one for up to
//! [`KEEP_FOR`] ([`Known::merge`]).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
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

/// The answers behind a route, with when a real lookup last gave each (a monotonic reading).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Known {
    pub(crate) answers: Answered,
    confirmed: BTreeMap<Url, Duration>,
}

impl Known {
    /// `fresh`, got at `now`, over these. A real answer is taken, confirmed now. An unknown one
    /// keeps this route's answer for its target when `fresh` asks about the same targets (a
    /// refresh, or settings that leave them) and that answer was confirmed under `keep_for` ago:
    /// a proxy survives a transient failure, and only an explicit `DIRECT` takes it away. Else
    /// it stays unknown, so that target goes direct (a first route, other hosts, or too old).
    pub(crate) fn merge(&self, fresh: Answered, now: Duration, keep_for: Duration) -> Known {
        let same = self.answers.keys().eq(fresh.keys());
        let mut known = Known::default();
        for (target, answer) in fresh {
            let kept = (answer == OsAnswer::Unknown && same)
                .then(|| Some((self.answers.get(&target)?, *self.confirmed.get(&target)?)))
                .flatten()
                .filter(|(_, at)| now.saturating_sub(*at) < keep_for);
            match kept {
                Some((kept, at)) => {
                    known.answers.insert(target.clone(), kept.clone());
                    known.confirmed.insert(target, at);
                }
                None if answer == OsAnswer::Unknown => {
                    known.answers.insert(target, answer);
                }
                None => {
                    known.answers.insert(target.clone(), answer);
                    known.confirmed.insert(target, now);
                }
            }
        }
        known
    }

    /// With the real answers in `late` (lookups past the budget that landed at `now`) taken.
    pub(crate) fn landed(mut self, late: Answered, now: Duration) -> Known {
        for (target, answer) in late {
            if answer != OsAnswer::Unknown {
                self.answers.insert(target.clone(), answer);
                self.confirmed.insert(target, now);
            }
        }
        self
    }
}

/// `os`'s answer for each of `targets`, all asked at once, each on its own thread: what has
/// not answered within `budget` is [`OsAnswer::Unknown`] in this route. A lookup still running
/// is left to end on its thread (the sandbox bounds each one); its answer comes in
/// [`Answers::late`].
pub(crate) fn ask_all(targets: BTreeSet<Url>, os: &Arc<OsLookup>, budget: Duration) -> Answers {
    let clock = SystemClock;
    let deadline = clock.monotonic() + budget;
    let (send, answers) = std::sync::mpsc::channel();
    let mut asked = 0;
    let mut got: Answered = targets
        .iter()
        .map(|target| (target.clone(), OsAnswer::Unknown))
        .collect();
    for target in targets {
        let (os, send) = (Arc::clone(os), send.clone());
        let spawned = std::thread::Builder::new()
            .name("clauseo-proxy-lookup".to_owned())
            .spawn(move || {
                let answer = os(&target);
                // The route may be made already: then no one reads it.
                let _sent = send.send((target, answer));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "an OS proxy lookup did not start; it has no answer");
        } else {
            asked += 1;
        }
    }
    let mut answered = 0;
    while answered < asked {
        let left = deadline.saturating_sub(clock.monotonic());
        let Ok((target, answer)) = answers.recv_timeout(left) else {
            tracing::warn!(
                late = asked - answered,
                "OS proxy lookups past the budget have no answer in this route"
            );
            let late = Late {
                running: asked - answered,
                answers,
            };
            return Answers {
                got,
                late: Some(late),
            };
        };
        got.insert(target, answer);
        answered += 1;
    }
    Answers { got, late: None }
}

/// The OS's answers [`ask_all`] got within its budget, and the lookups still running past it.
pub(crate) struct Answers {
    /// Each target's answer, [`OsAnswer::Unknown`] for one not in time.
    pub(crate) got: Answered,
    /// The lookups past the budget, when any.
    pub(crate) late: Option<Late>,
}

/// Lookups still running past the budget: each one's answer comes on `answers` as it ends.
pub(crate) struct Late {
    pub(crate) running: usize,
    pub(crate) answers: std::sync::mpsc::Receiver<(Url, OsAnswer)>,
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
