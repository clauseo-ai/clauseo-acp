//! A failed model probe (`actor/agent_options/probe.rs`): the agent's `models_unread` says the
//! list could not be read while nothing fills its models, the next good limits read probes
//! again, and a probe or a session that fills the models clears the mark.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use clauseo_limits::{ReadLimits, ReadOutcome};
use clauseo_protocol::{
    AgentId, AgentKind, AgentModels, AgentSummary, CommandOutcome, HostCommand, HostEvent,
    Principal, RefreshWhy,
};

use crate::limits_host::refresh;
use crate::pickers::{CLAUDE, turn};
use crate::pickers_seed::agent_summary;
use crate::support::{Dir, Running, Scenario, wait};

/// A reader whose reads find the agent signed in and whose probes answer `answers` in turn,
/// the last one again once they run out, counting the probes.
struct SeqReader {
    answers: Mutex<VecDeque<Option<AgentModels>>>,
    probed: async_channel::Sender<()>,
}

impl ReadLimits for SeqReader {
    fn read(&self, _agent: AgentKind) -> Pin<Box<dyn Future<Output = ReadOutcome> + Send>> {
        Box::pin(std::future::ready(ReadOutcome::MethodMissing))
    }

    fn probe_models(
        &self,
        _agent: AgentKind,
    ) -> Pin<Box<dyn Future<Output = Option<AgentModels>> + Send>> {
        let mut answers = self.answers.lock().unwrap();
        let models = if answers.len() > 1 {
            answers.pop_front().flatten()
        } else {
            answers.front().cloned().flatten()
        };
        let probed = self.probed.clone();
        Box::pin(async move {
            let _gone = probed.try_send(());
            models
        })
    }
}

fn start(dir: &Dir, answers: Vec<Option<AgentModels>>) -> (Running, async_channel::Receiver<()>) {
    let (probed, probes) = async_channel::unbounded();
    let reader = SeqReader {
        answers: Mutex::new(answers.into()),
        probed,
    };
    let host = dir.start_with(0, |config| config.with_limits_reader(Arc::new(reader)));
    (host, probes)
}

/// Asks `ListAgents` until `done` holds of Claude's summary, again after each settings change.
pub(crate) fn summary_when(host: &Running, done: impl Fn(&AgentSummary) -> bool) -> AgentSummary {
    use futures::StreamExt as _;
    let subscribed = host.handle.subscribe_app(Principal::LocalDesktop);
    let mut sub = wait("subscribe_app", subscribed).unwrap();
    loop {
        let summary = agent_summary(host, CLAUDE);
        if done(&summary) {
            return summary;
        }
        wait("a settings change", async {
            loop {
                let item = sub.next().await.expect("the app stream ended");
                if matches!(
                    crate::settings::decode(&item),
                    HostEvent::SettingsChanged(_)
                ) {
                    break;
                }
            }
        });
    }
}

#[test]
fn a_failed_probe_marks_the_models_unread_and_the_next_read_probes_again() {
    let dir = Dir::new(
        "pickers_probe_retry",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, probes) = start(&dir, vec![None, Some(crate::pickers_probe::probed())]);
    wait("the failed probe", probes.recv()).unwrap();
    let unread = summary_when(&host, |summary| {
        summary.models_unread || summary.models.is_some()
    });
    assert!(unread.models_unread, "{unread:?}");
    assert_eq!(unread.models, None);
    // The next good read probes again (a try counts past 10 s from the last read's start);
    // its answer fills the models and clears the mark.
    host.clock.advance(std::time::Duration::from_secs(11));
    refresh(&host, RefreshWhy::TryAgain);
    wait("the second probe", probes.recv()).unwrap();
    let read = summary_when(&host, |summary| summary.models.is_some());
    assert!(!read.models_unread);
    assert_eq!(read.models, Some(crate::pickers_probe::probed()));
    host.shutdown();
}

#[test]
fn a_session_that_fills_the_models_clears_the_unread_mark() {
    let dir = Dir::new(
        "pickers_probe_unread_session",
        &[(CLAUDE, Scenario::Bundled("pickers-seed"))],
    );
    let (host, probes) = start(&dir, vec![None]);
    wait("the failed probe", probes.recv()).unwrap();
    summary_when(&host, |summary| summary.models_unread);
    let folder = dir.folder("m", &[("brief.txt", "x\n")]);
    let matter = host.create_matter("m", &folder);
    let outcome = host.ok(HostCommand::CreateThread {
        matter,
        agent: AgentId::new(CLAUDE).unwrap(),
        config: None,
    });
    let CommandOutcome::ThreadCreated { thread } = outcome else {
        panic!("CreateThread answered {outcome:?}");
    };
    turn(&host, thread, "Summarise clause 4.");
    let filled = summary_when(&host, |summary| summary.models.is_some());
    assert!(!filled.models_unread, "{filled:?}");
    host.shutdown();
}
