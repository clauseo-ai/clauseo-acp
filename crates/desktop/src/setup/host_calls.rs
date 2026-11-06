//! The host's Rust-only calls the setup backend makes (decision record 0048), behind one trait,
//! [`SetupHost`], so the backend's tests record them (`RecordingHost` in `tests.rs`) and the
//! app goes through the bridge's [`HostCell`] ([`CellHost`]).
//!
//! Piece A calls [`SetupHost::set_agent_launch`] (an install's `Ready`) and
//! [`SetupHost::set_endpoint`] (`SetFirmEndpoint`); piece B the leave order's calls, the
//! restarts, `limits_login_changed` (P4d), the launch refusal for an unreadable policy (P4d),
//! the splits (P4b) and the settings changes, which come from an app subscription.

use std::future::ready;
use std::sync::Arc;

use clauseo_host::{AgentsStopped, LoginSplits, StopFor};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode, HostError, Principal};
use clauseo_settings::{AgentLaunch, EntryWrite};
use futures::StreamExt as _;
use futures::stream::BoxStream;

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

    /// Sets or removes `agent`'s `plans.<id>.endpoint`, keeping the plan's provider and
    /// variables (`HostHandle::set_plan_endpoint`).
    fn set_endpoint(
        &self,
        agent: AgentId,
        endpoint: Option<String>,
    ) -> BoxFuture<'_, Result<(), HostError>>;

    /// Stops each live agent of that id not in a turn and resumes the queues held for its
    /// sign-in (P4b's `restart_agents`).
    fn restart_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Cancels any turn of that id, keeping it held, and stops its live agents (P4b's
    /// `stop_agents`), answering whether they exited or the bounded wait ran out (P4d).
    /// The leave order's first step.
    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<AgentsStopped, HostError>>;

    /// [`Self::stop_agents`] with the reason the held queues show (`stop_agents_for`): R17's
    /// account change stops with `StopFor::SignInAgain`, so its held turns show `sign_in` and
    /// stay held until the new sign-in's `restart_agents`.
    fn stop_agents_for(
        &self,
        agent: AgentId,
        why: StopFor,
    ) -> BoxFuture<'_, Result<AgentsStopped, HostError>>;

    /// Tells the host whether `agent`'s login is the lawyer's own, reused (P4b's
    /// `set_reused_login`).
    fn set_reused_login(
        &self,
        agent: AgentId,
        reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>>;

    /// Removes each matter's link to the agent's login (P4b's `remove_login_links`).
    fn remove_login_links(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Has `limits` read the agent's account now, past its back-off (0048's Rust-only
    /// `LoginChanged`; P4b's follow-up).
    fn login_changed(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>>;

    /// Refuses `agent`'s new launches with `code`, or lifts the refusal with `None` (P4d's
    /// `set_launch_refused`): `signin.policy_unreadable` while its policy blocks it.
    fn set_launch_refused(
        &self,
        agent: AgentId,
        code: Option<ErrorCode>,
    ) -> BoxFuture<'_, Result<(), HostError>>;

    /// The splits the host's check finds (P4b's `login_splits`).
    fn login_splits(&self) -> BoxFuture<'_, Result<LoginSplits, HostError>>;

    /// One item per `SettingsChanged`, the current settings' first, from an app subscription;
    /// it ends when the host stops.
    fn settings_changes(&self) -> BoxFuture<'_, Result<BoxStream<'static, ()>, HostError>>;
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

/// Whether `json` is a `HostEvent::SettingsChanged` (external tagging, snake case: its one
/// key comes first), read from the text so the other app events are not decoded.
fn is_settings_changed(json: &str) -> bool {
    json.trim_start()
        .strip_prefix('{')
        .is_some_and(|rest| rest.trim_start().starts_with("\"settings_changed\""))
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

    fn set_endpoint(
        &self,
        agent: AgentId,
        endpoint: Option<String>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move {
            self.cell
                .host()
                .await?
                .set_plan_endpoint(agent, endpoint)
                .await
        })
    }

    fn restart_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move { self.cell.host().await?.restart_agents(agent).await })
    }

    fn stop_agents(&self, agent: AgentId) -> BoxFuture<'_, Result<AgentsStopped, HostError>> {
        Box::pin(async move { self.cell.host().await?.stop_agents(agent).await })
    }

    fn stop_agents_for(
        &self,
        agent: AgentId,
        why: StopFor,
    ) -> BoxFuture<'_, Result<AgentsStopped, HostError>> {
        Box::pin(async move { self.cell.host().await?.stop_agents_for(agent, why).await })
    }

    fn set_reused_login(
        &self,
        agent: AgentId,
        reused: bool,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move {
            self.cell
                .host()
                .await?
                .set_reused_login(agent, reused)
                .await
        })
    }

    fn remove_login_links(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move { self.cell.host().await?.remove_login_links(agent).await })
    }

    fn login_changed(&self, agent: AgentId) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move {
            let Some(kind) = AgentKind::of(&agent) else {
                return Ok(());
            };
            self.cell.host().await?.limits_login_changed(kind).await
        })
    }

    fn set_launch_refused(
        &self,
        agent: AgentId,
        code: Option<ErrorCode>,
    ) -> BoxFuture<'_, Result<(), HostError>> {
        Box::pin(async move {
            self.cell
                .host()
                .await?
                .set_launch_refused(agent, code)
                .await
        })
    }

    fn login_splits(&self) -> BoxFuture<'_, Result<LoginSplits, HostError>> {
        Box::pin(async move { Ok(self.cell.host().await?.login_splits()) })
    }

    fn settings_changes(&self) -> BoxFuture<'_, Result<BoxStream<'static, ()>, HostError>> {
        Box::pin(async move {
            let host = self.cell.host().await?;
            let events = host.subscribe_app(Principal::LocalDesktop).await?;
            let changes =
                events.filter_map(|event| ready(is_settings_changed(event.json()).then_some(())));
            Ok(changes.boxed())
        })
    }
}
