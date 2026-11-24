//! A model probe across a login change (`actor/agent_options/probe.rs`): the answer or failure
//! of a probe started before `limits_login_changed` is ignored, so the new login's probe fills
//! the list and the old one uses none of the new login's tries. (The login change's hold ends
//! the old probe's process: `clauseo_limits`' `tests/readers/models.rs`.)

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clauseo_limits::{ReadLimits, ReadOutcome};
use clauseo_protocol::{AgentKind, AgentModels, RefreshWhy};
use futures::channel::oneshot;

use crate::limits_host::refresh;
use crate::pickers::CLAUDE;
use crate::pickers_probe::probed;
use crate::pickers_probe_retry::summary_when;
use crate::pickers_seed::agent_summary;
use crate::support::{Dir, Running, Scenario, wait};

/// A reader whose reads find the agent signed in and whose probes each wait for the answer the
/// test sends; it says when each probe starts and when each returns.
struct GatedReader {
    answers: Mutex<VecDeque<oneshot::Receiver<Option<AgentModels>>>>,
    started: async_channel::Sender<()>,
    returned: async_channel::Sender<()>,
}

impl ReadLimits for GatedReader {
    fn read(&self, _agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        Box::pin(std::future::ready(ReadOutcome::MethodMissing))
    }

    fn probe_models(
        &self,
        _agent: AgentKind,
    ) -> Pin<Box<dyn Future<Output = Option<AgentModels>> + Send>> {
        let answer = self.answers.lock().unwrap().pop_front();
        let started = self.started.clone();
        let returned = self.returned.clone();
        Box::pin(async move {
            let _gone = started.try_send(());
            let models = match answer {
                Some(answer) => answer.await.ok().flatten(),
                None => None,
            };
            // The host posts the answer in this same poll.
            let _gone = returned.try_send(());
            models
        })
    }
}

/// The test's side: each probe's answer, and the reader's signals.
struct Probes {
    answers: VecDeque<oneshot::Sender<Option<AgentModels>>>,
    started: async_channel::Receiver<()>,
    returned: async_channel::Receiver<()>,
}

impl Probes {
    /// Waits for the next probe to start.
    fn started(&self, what: &str) {
        wait(what, self.started.recv()).unwrap();
    }

    /// Answers the oldest probe not answered yet, and waits until the host has its answer.
    fn answer(&mut self, models: Option<AgentModels>) {
        let answer = self.answers.pop_front().expect("a probe left to answer");
        answer.send(models).unwrap();
        wait("the probe's return", self.returned.recv()).unwrap();
    }
}

fn start(dir: &Dir, probes: usize) -> (Running, Probes) {
    let (senders, receivers): (VecDeque<_>, VecDeque<_>) =
        (0..probes).map(|_| oneshot::channel()).unzip();
    let (started, started_rx) = async_channel::unbounded();
    let (returned, returned_rx) = async_channel::unbounded();
    let reader = GatedReader {
        answers: Mutex::new(receivers),
        started,
        returned,
    };
    let host = dir.start_with(0, |config| config.with_limits_reader(Arc::new(reader)));
    let probes = Probes {
        answers: senders,
        started: started_rx,
        returned: returned_rx,
    };
    (host, probes)
}

/// The old login's list: the probed one less its last model.
fn old_login() -> AgentModels {
    let mut models = probed();
    models.models.pop();
    models
}

fn login_changed(host: &Running) {
    let changed = host.handle.limits_login_changed(AgentKind::Claude);
    wait("limits_login_changed", changed).unwrap();
}

#[test]
fn a_probe_from_before_a_sign_in_is_ignored_and_the_new_logins_probe_fills_the_list() {
    let dir = Dir::new(
        "pickers_probe_login",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, mut probes) = start(&dir, 2);
    probes.started("the first probe");
    // A sign-in while it runs: the new login's read probes again.
    login_changed(&host);
    probes.started("the new login's probe");
    let first = probes.answers.pop_front().unwrap();
    probes.answer(Some(probed()));
    let filled = summary_when(&host, |summary| summary.models.is_some());
    assert_eq!(filled.models, Some(probed()));
    // The old login's probe answers late: ignored.
    probes.answers.push_front(first);
    probes.answer(Some(old_login()));
    // Its answer is posted; anything the host posts after it comes after it. Another login
    // change's read (posted once the host has the command) starts a probe once handled.
    login_changed(&host);
    probes.started("the next login's probe");
    let summary = agent_summary(&host, CLAUDE);
    assert_eq!(
        summary.models,
        Some(probed()),
        "the old login's list came back"
    );
    host.shutdown();
}

#[test]
fn a_failure_from_before_a_sign_in_uses_none_of_the_new_logins_tries() {
    let dir = Dir::new(
        "pickers_probe_login_failure",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, mut probes) = start(&dir, 4);
    probes.started("the first probe");
    login_changed(&host);
    probes.started("the new login's probe");
    // The old login's probe fails first: it uses no try.
    probes.answer(None);
    for probe in ["the second try", "the third try"] {
        probes.answer(None);
        host.clock.advance(Duration::from_secs(11));
        refresh(&host, RefreshWhy::TryAgain);
        probes.started(probe);
    }
    probes.answer(None);
    let unread = summary_when(&host, |summary| summary.models_unread);
    assert_eq!(unread.models, None);
    host.shutdown();
}
