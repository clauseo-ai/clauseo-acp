//! The OS asked about every target at once, within one budget ([`ask_all`]), and the lookups
//! still running past it ([`Late`]), whose answers make the route again once they land. Each
//! answer is a proxy, an explicit `DIRECT`, or unknown ([`OsAnswer`]): a failed, late or
//! slot-refused lookup is no answer, so a refresh keeps the earlier one ([`kept_over_unknown`]).

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

/// `fresh` with each unknown answer replaced by `prior`'s for that target, when it has one: a
/// refresh of the same read keeps a proxy over a transient failure, and only an explicit
/// `DIRECT` takes it away. With no `prior` (a first route, or after the settings changed), an
/// unknown answer stays unknown, so that target goes direct.
pub(crate) fn kept_over_unknown(mut fresh: Answered, prior: Option<&Answered>) -> Answered {
    for (target, answer) in &mut fresh {
        if *answer == OsAnswer::Unknown
            && let Some(kept) = prior.and_then(|prior| prior.get(target))
        {
            answer.clone_from(kept);
        }
    }
    fresh
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
