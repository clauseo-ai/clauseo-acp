//! The host's Rust-only calls `RecordingHost` records, each answered as a test set it.

use std::sync::Mutex;

use clauseo_host::{AgentsStopped, LoginSplits, StopFor};
use clauseo_protocol::{AgentId, ErrorCode, HostError};
use clauseo_settings::{AgentLaunch, EntryWrite};
use futures::stream::BoxStream;

use super::super::install_busy::{LaunchGate, held_write};
use super::{HostCall, Journal};
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::setup::host_calls::SetupHost;

/// The host's Rust-only calls, recorded in order, each answered `Ok`; `stop_agents` answers
/// `stopped` (`Exited` unless a test sets `TimedOut`), and the next `restart_fails`
/// `restart_agents` calls answer `internal`.
pub(in crate::setup::backend::tests) struct RecordingHost {
    pub(in crate::setup::backend::tests) calls: Mutex<Vec<HostCall>>,
    pub(in crate::setup::backend::tests) journal: Journal,
    pub(in crate::setup::backend::tests) stopped: Mutex<AgentsStopped>,
    pub(in crate::setup::backend::tests) restart_fails: Mutex<usize>,
    /// The next `set_launch_refused` calls that answer `internal` (recorded all the same).
    pub(in crate::setup::backend::tests) refused_fails: Mutex<usize>,
    /// The next `stop_agents` panics, once a test sets it.
    pub(in crate::setup::backend::tests) stop_panics: Mutex<bool>,
    /// The next `stop_agents` answers once this opens, when a test sets it.
    pub(in crate::setup::backend::tests) stop_gate:
        Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
    /// What `settings_changes` answers, once a test sets it.
    pub(in crate::setup::backend::tests) changes: Mutex<Option<BoxStream<'static, ()>>>,
    /// Told at each `restart_agents`, once a test sets it.
    pub(in crate::setup::backend::tests) restarted: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// What `sign_in_holds` answers, once a test sets it.
    pub(in crate::setup::backend::tests) holds: Mutex<Option<BoxStream<'static, AgentId>>>,
    /// The next `set_agent_launch` tells the sender, then answers once the receiver opens.
    pub(in crate::setup::backend::tests) launch_gate: Mutex<Option<LaunchGate>>,
}

impl RecordingHost {
    pub(in crate::setup::backend::tests) fn new(journal: Journal) -> Self {
        Self {
            calls: Mutex::default(),
            journal,
            stopped: Mutex::new(AgentsStopped::Exited),
            restart_fails: Mutex::new(0),
            refused_fails: Mutex::new(0),
            stop_panics: Mutex::new(false),
            stop_gate: Mutex::new(None),
            changes: Mutex::new(None),
            restarted: Mutex::new(None),
            holds: Mutex::new(None),
            launch_gate: Mutex::new(None),
        }
    }

    pub(in crate::setup::backend::tests) fn calls(&self) -> Vec<HostCall> {
        lock(&self.calls).clone()
    }

    fn push(&self, call: HostCall) {
        let name = format!("{call:?}");
        let name = name.split('(').next().unwrap_or_default();
        lock(&self.journal).push(format!("host.{name}"));
        lock(&self.calls).push(call);
    }

    fn record(&self, call: HostCall) -> BoxFuture<'_, Result<(), HostError>> {
        self.push(call);
        Box::pin(std::future::ready(Ok(())))
    }
}

impl SetupHost for RecordingHost {
    fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: AgentLaunch,
    ) -> BoxFuture<'_, Result<EntryWrite, HostError>> {
        self.push(HostCall::SetAgentLaunch(agent, launch));
        Box::pin(held_write(lock(&self.launch_gate).take()))
    }
    fn set_endpoint(
        &self,
        agent: AgentId,
        endpoint: Option<String>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::SetEndpoint(agent, endpoint))
    }
    fn restart_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.push(HostCall::RestartAgents(agent));
        if let Some(told) = &*lock(&self.restarted) {
            let _told = told.send(());
        }
        let mut fails = lock(&self.restart_fails);
        let answer = if *fails > 0 {
            *fails -= 1;
            Err(HostError::new(ErrorCode::Internal, "the restart failed"))
        } else {
            Ok(())
        };
        Box::pin(std::future::ready(answer))
    }
    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<AgentsStopped, HostError>> {
        self.push(HostCall::StopAgents(agent));
        let panics = std::mem::take(&mut *lock(&self.stop_panics));
        assert!(!panics, "the test's stop_agents panics");
        let gate = lock(&self.stop_gate).take();
        let stopped = *lock(&self.stopped);
        Box::pin(async move {
            if let Some(gate) = gate {
                let _opened = gate.await;
            }
            Ok(stopped)
        })
    }
    fn stop_agents_for(
        &self,
        agent: AgentId,
        why: StopFor,
    ) -> BoxFuture<'_, Result<AgentsStopped, HostError>> {
        self.push(HostCall::StopAgentsFor(agent, why));
        let stopped = *lock(&self.stopped);
        Box::pin(std::future::ready(Ok(stopped)))
    }
    fn set_reused_login(
        &self,
        agent: AgentId,
        reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::SetReusedLogin(agent, reused))
    }
    fn remove_login_links(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::RemoveLoginLinks(agent))
    }
    fn login_changed(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        self.record(HostCall::LoginChanged(agent))
    }
    fn set_launch_refused(
        &self,
        agent: AgentId,
        code: Option<ErrorCode>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        let failing = {
            let mut fails = lock(&self.refused_fails);
            let failing = *fails > 0;
            *fails = fails.saturating_sub(1);
            failing
        };
        if failing {
            self.push(HostCall::SetLaunchRefused(agent, code));
            let error = HostError::new(ErrorCode::Internal, "the refusal was not set");
            return Box::pin(std::future::ready(Err(error)));
        }
        self.record(HostCall::SetLaunchRefused(agent, code))
    }
    fn login_splits(&self) -> BoxFuture<'_, Result<LoginSplits, HostError>> {
        Box::pin(std::future::ready(Err(HostError::new(
            ErrorCode::Internal,
            "the tests feed no splits",
        ))))
    }
    fn settings_changes(&self) -> BoxFuture<'_, Result<BoxStream<'static, ()>, HostError>> {
        lock(&self.journal).push("host.settings_changes".to_owned());
        let answer = lock(&self.changes).take().ok_or_else(|| {
            HostError::new(ErrorCode::Internal, "the tests apply settings directly")
        });
        Box::pin(std::future::ready(answer))
    }
    fn sign_in_holds(&self) -> BoxFuture<'_, Result<BoxStream<'static, AgentId>, HostError>> {
        let answer = lock(&self.holds)
            .take()
            .ok_or_else(|| HostError::new(ErrorCode::Internal, "the tests feed no holds"));
        Box::pin(std::future::ready(answer))
    }
}
