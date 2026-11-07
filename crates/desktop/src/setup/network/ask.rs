//! The OS asked about every target at once, within one budget ([`ask_all`]), and the lookups
//! still running past it ([`Late`]), whose answers make the route again once they land.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::{Clock as _, SystemClock};
use url::Url;

use super::OsLookup;

/// `os`'s answer for each of `targets`, all asked at once, each on its own thread: what has
/// not answered within `budget` counts as none (direct) in this route. A lookup still running
/// is left to end on its thread (the sandbox bounds each one); its answer comes in
/// [`Answers::late`].
pub(crate) fn ask_all(targets: BTreeSet<Url>, os: &Arc<OsLookup>, budget: Duration) -> Answers {
    let clock = SystemClock;
    let deadline = clock.monotonic() + budget;
    let (send, answers) = std::sync::mpsc::channel();
    let mut asked = 0;
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
            tracing::warn!(%err, "an OS proxy lookup did not start; it goes direct");
        } else {
            asked += 1;
        }
    }
    let mut got = BTreeMap::new();
    while got.len() < asked {
        let left = deadline.saturating_sub(clock.monotonic());
        let Ok((target, answer)) = answers.recv_timeout(left) else {
            tracing::warn!(
                late = asked - got.len(),
                "OS proxy lookups past the budget count as direct for this route"
            );
            let late = Late {
                running: asked - got.len(),
                answers,
            };
            return Answers {
                got,
                late: Some(late),
            };
        };
        got.insert(target, answer);
    }
    Answers { got, late: None }
}

/// The OS's answers [`ask_all`] got within its budget, and the lookups still running past it.
pub(crate) struct Answers {
    /// Each target's answer in time.
    pub(crate) got: BTreeMap<Url, Option<String>>,
    /// The lookups past the budget, when any.
    pub(crate) late: Option<Late>,
}

/// Lookups still running past the budget: each one's answer comes on `answers` as it ends.
pub(crate) struct Late {
    pub(crate) running: usize,
    pub(crate) answers: std::sync::mpsc::Receiver<(Url, Option<String>)>,
}

impl Late {
    /// The late answers that end within `wait` (the sandbox bounds each lookup at 10 s).
    pub(crate) fn collect(self, wait: Duration) -> BTreeMap<Url, Option<String>> {
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
