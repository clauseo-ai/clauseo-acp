//! The host's Rust-only calls the setup backend makes (decision record 0048), behind one trait,
//! [`SetupHost`], so the backend's tests record them (`RecordingHost` in `tests.rs`) and the
//! app goes through the bridge's [`HostCell`] ([`CellHost`]).
//!
//! R20 P5b piece A calls [`SetupHost::set_agent_launch`] (an install's `Ready`) and
//! [`SetupHost::set_plan`] (`SetFirmEndpoint`). The others are piece B's (the leave order, the
//! restarts, `LoginChanged`). This host has none of them yet (P4b, #440, adds `restart_agents`,
//! `stop_agents`, `set_reused_login` and `remove_login_links`; `login_changed` is P4b's
//! follow-up), so [`CellHost`] answers each with `internal` until they land.

use std::sync::Arc;

use clauseo_protocol::{AgentId, ErrorCode, HostError};
use clauseo_settings::{AgentLaunch, EntryWrite, Plan};

use crate::dialogs::BoxFuture;
use crate::host_start::HostCell;

/// What the setup backend asks of the host. Every call is Rust only: no window reaches one.
pub(crate) trait SetupHost: Send + Sync {
    /// Writes `agent`'s `agents.<id>` entry (`HostHandle::set_agent_launch`).
    fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: AgentLaunch,
    ) -> BoxFuture<'_, Result<EntryWrite, HostError>>;

    /// Writes or removes `agent`'s `plans.<id>` (`HostHandle::set_plan`).
    fn set_plan(&self, agent: AgentId, plan: Option<Plan>) -> BoxFuture<'_, Result<(), HostError>>;

    /// Stops each live agent of that id not in a turn and resumes the queues held for its
    /// sign-in (P4b's `restart_agents`). Piece B.
    #[expect(dead_code, reason = "R20 P5b piece B calls it")]
    fn restart_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Cancels any turn of that id, keeping it held, and stops its live agents (P4b's
    /// `stop_agents`). Piece B: the leave order's first step.
    #[expect(dead_code, reason = "R20 P5b piece B calls it")]
    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Tells the host whether `agent`'s login is the lawyer's own, reused (P4b's
    /// `set_reused_login`). Piece B.
    #[expect(dead_code, reason = "R20 P5b piece B calls it")]
    fn set_reused_login(
        &self,
        agent: AgentId,
        reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>>;

    /// Removes each matter's link to the agent's login (P4b's `remove_login_links`). Piece B.
    #[expect(dead_code, reason = "R20 P5b piece B calls it")]
    fn remove_login_links(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Has `limits` read the agent's account now, past its back-off (0048's Rust-only
    /// `LoginChanged`; P4b's follow-up). Piece B.
    #[expect(dead_code, reason = "R20 P5b piece B calls it")]
    fn login_changed(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;
}

/// The app's [`SetupHost`]: each call waits for the host's start in the bridge's cell.
pub(crate) struct CellHost {
    cell: Arc<HostCell>,
}

impl CellHost {
    /// Calls on the host whose start lands in `cell`.
    pub(crate) fn new(cell: Arc<HostCell>) -> Self {
        Self { cell }
    }
}

/// The answer to a call this host does not have yet: `internal`, naming the call.
fn not_in_host(call: &str) -> HostError {
    HostError::new(
        ErrorCode::Internal,
        format!("the host has no {call} yet (R20 P4b)"),
    )
}

impl SetupHost for CellHost {
    fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: AgentLaunch,
    ) -> BoxFuture<'_, Result<EntryWrite, HostError>> {
        Box::pin(async move {
            self.cell
                .host()
                .await?
                .set_agent_launch(agent, launch)
                .await
        })
    }

    fn set_plan(&self, agent: AgentId, plan: Option<Plan>) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move { self.cell.host().await?.set_plan(agent, plan).await })
    }

    fn restart_agents(&self, _agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(std::future::ready(Err(not_in_host("restart_agents"))))
    }

    fn stop_agents(&self, _agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(std::future::ready(Err(not_in_host("stop_agents"))))
    }

    fn set_reused_login(
        &self,
        _agent: AgentId,
        _reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(std::future::ready(Err(not_in_host("set_reused_login"))))
    }

    fn remove_login_links(&self, _agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(std::future::ready(Err(not_in_host("remove_login_links"))))
    }

    fn login_changed(&self, _agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(std::future::ready(Err(not_in_host("limits_login_changed"))))
    }
}
