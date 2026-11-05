//! The setup backend over S18's `Agents` (R20 P5b piece A): [`AgentsBackend`] answers a
//! window's [`SetupCommand`] with `Agents`, the [`KeyCache`], the host's Rust-only calls
//! ([`SetupHost`]) and the native confirms ([`SetupConfirm`]), and its
//! [`pump`](AgentsBackend::pump) turns `Agents`' events into [`SetupEvent`]s.
//!
//! - **Installs.** Each `AgentsEvent::Install` goes out as `AgentInstallProgress`, with the hosts
//!   the install contacts for "Ask your IT team". On `Ready` the agent's `agents.<id>` entry is
//!   built from the install ([`launch`]) and written with `set_agent_launch`: a webview never
//!   chooses a program to run. First run: once every agent has its first state, with none
//!   installed and none `Managed`, Claude's install starts with no command and no confirm.
//! - **Sign-in.** Each `AgentsEvent::Signin` goes to [`Compose`], which sends every
//!   `SignInChanged`. `OpenUrl` (an address S18's shim captured and checked against the
//!   snapshot's sign-in hosts) opens with `open::open_url_unconfirmed`, https only. A
//!   `SigninNote` is logged by its code (`events::note`). The status check runs once an agent's
//!   install is `Ready`; an agent not installed shows `SignedOut`.
//! - **Confirms.** `InstallAgent` and `SetFirmEndpoint` ask the lawyer first; a decline runs
//!   nothing and answers `transport.unauthorized`. `RetryAgentInstall` asks nothing, so it is
//!   `command.invalid` unless the install is `Failed`, `Waiting` or `Paused`: a missing agent
//!   is installed only through `InstallAgent`'s confirm. `OpenSignInPage` opens the `DeviceCode`
//!   state's page and does nothing in any other state. `SignOut` on a `Reused` login answers
//!   `command.invalid` and runs no logout.
//! - **Piece B's.** Leaving a reused login runs 0048's leave order (stop the agents, S18's
//!   `stop_using_login`, unlink, relaunch), which needs the host calls P4b adds: until then
//!   `StopUsingLogin`, and a key or an endpoint set while the login is reused, answer
//!   `internal` and run nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clauseo_agents::{
    AgentsEvent, InstallState, Method, PasteCode, SigninError, SigninState, Source,
};
use clauseo_protocol::{
    AgentId, AgentKind, CommandOutcome, ErrorCode, HostError, SetupCommand, SetupEvent,
    SignInSource,
};
use clauseo_settings::EntryWrite;
use futures::{Stream, StreamExt as _};

use super::admit::sign_in_barred;
use super::compose::Compose;
use super::confirm::{Ask, SetupConfirm};
use super::host_calls::SetupHost;
use super::keys::KeyCache;
use super::{KeyWrite, SetupBackend, SetupEvents, events};
use crate::bridge::lock;
use crate::dialogs::BoxFuture;
use crate::open::{Opener, open_url_unconfirmed};

pub(crate) use self::entry::{launch, progress};
pub(crate) use self::seam::SetupAgents;

/// One agent as the backend knows it from the snapshot and the locations.
#[derive(Debug, Clone)]
pub(crate) struct AgentInfo {
    /// Its name, for the confirms.
    pub(crate) name: String,
    /// The hosts its install contacts, for "Ask your IT team".
    pub(crate) hosts: Vec<String>,
    /// Its home, `Locations::agent_home`.
    pub(crate) home: PathBuf,
    /// Whether policy sets its `plans.<id>` (`SetFirmEndpoint` is then `settings.locked`).
    pub(crate) plan_locked: bool,
    /// Whether its plan names a provider, which keeps it on the firm's service with no endpoint.
    pub(crate) provider: bool,
}

/// The agent installed at first run when nothing is installed and nothing is managed.
pub(crate) const FIRST_RUN_AGENT: &str = "claude-acp";

/// The setup backend over `Agents`.
pub(crate) struct AgentsBackend {
    pub(crate) agents: Arc<dyn SetupAgents>,
    pub(crate) keys: Arc<KeyCache>,
    pub(crate) compose: Arc<Compose>,
    pub(crate) host: Arc<dyn SetupHost>,
    pub(crate) confirm: Arc<dyn SetupConfirm>,
    pub(crate) opener: Arc<dyn Opener>,
    pub(crate) events: Arc<SetupEvents>,
    /// The agents Clauseo sets up, by id.
    pub(crate) known: BTreeMap<AgentId, AgentInfo>,
    /// Held from a sign-in start's check to its call, and from a key's or an endpoint's end of
    /// a running sign-in to its cancel, so a start never slips past a key or an endpoint.
    pub(crate) signin_gate: Mutex<()>,
}

impl SetupBackend for AgentsBackend {
    fn run<'a>(
        &'a self,
        command: SetupCommand,
        key_write: KeyWrite,
        _events: &'a SetupEvents,
    ) -> BoxFuture<'a, Result<CommandOutcome, HostError>> {
        Box::pin(async move {
            self.answer(command, key_write)
                .await
                .map(|()| CommandOutcome::Done)
        })
    }

    fn sign_in_refused(&self, agent: &AgentId) -> Option<HostError> {
        self.compose
            .sign_in_bar(agent)
            .map(|bar| sign_in_barred(agent, bar))
    }
}

impl AgentsBackend {
    /// Runs `command`; the command, and any secret in it, is dropped when this returns.
    async fn answer(&self, command: SetupCommand, key_write: KeyWrite) -> Result<(), HostError> {
        let agent = command.agent().clone();
        let Some(info) = self.known.get(&agent) else {
            return Err(invalid(format!("{agent} is not an agent Clauseo sets up")));
        };
        let agents = &*self.agents;
        match command {
            SetupCommand::InstallAgent { .. } => {
                let megabytes = match agents.state(&agent) {
                    Some(InstallState::Missing { download_bytes }) => {
                        Some(download_bytes.div_ceil(1_000_000))
                    }
                    _ => None,
                };
                let name = info.name.clone();
                self.ask(Ask::Install { name, megabytes }).await?;
                agents.install(&agent);
            }
            SetupCommand::RetryAgentInstall {
                use_system_proxy, ..
            } => self.retry(&agent, use_system_proxy)?,
            SetupCommand::StartSignIn { console, .. } => {
                let _gate = lock(&self.signin_gate);
                self.refuse_start(&agent)?;
                let method = plan_method(&agent, console)?;
                agents
                    .sign_in(&agent, method)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::CancelSignIn { .. } => {
                agents
                    .cancel_sign_in(&agent)
                    .map_err(|error| signin_error(&error))?;
            }
            SetupCommand::PasteSignInCode { code, .. } => {
                let code = PasteCode::new(code.into_zeroizing());
                agents
                    .paste_code(&agent, code)
                    .map_err(|error| signin_error(&error))?;
            }
            SetupCommand::UseDeviceCode { .. } => {
                let _gate = lock(&self.signin_gate);
                self.refuse_start(&agent)?;
                agents
                    .sign_in(&agent, Method::DeviceCode)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::OpenSignInPage { .. } => {
                if let Some(SigninState::DeviceCode { url, .. }) = agents.signin_state(&agent) {
                    open_url_unconfirmed(&*self.opener, &url)?;
                }
            }
            SetupCommand::SignOut { .. } => {
                if self.reused(&agent) {
                    return Err(invalid(format!(
                        "{agent}'s login is the lawyer's own; Clauseo never signs it out"
                    )));
                }
                agents
                    .sign_out(&agent)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::UseFoundLogin { .. } => {
                let _gate = lock(&self.signin_gate);
                self.refuse_start(&agent)?;
                agents
                    .use_found_login(&agent)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::UseDifferentAccount { .. } => {
                let _gate = lock(&self.signin_gate);
                self.refuse_start(&agent)?;
                agents
                    .use_different_account(&agent)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::StopUsingLogin { .. } => return Err(leave_order_missing()),
            SetupCommand::FindLogin { .. } => {
                agents
                    .find_login(&agent)
                    .map_err(|error| signin_error(&error))?;
                self.replaced(&agent);
            }
            SetupCommand::SetAgentKey { key, .. } => {
                self.refuse_blocked(&agent)?;
                self.refuse_leaving_reused(&agent)?;
                let then = self.key_trigger(&agent);
                self.keys.set(&agent, key, key_write, then).await?;
                self.end_sign_in(&agent);
            }
            SetupCommand::ClearAgentKey { .. } => {
                let then = self.key_trigger(&agent);
                self.keys.clear(&agent, then).await?;
            }
            SetupCommand::SetFirmEndpoint { url, .. } => {
                self.set_endpoint(&agent, info, url).await?;
            }
        }
        Ok(())
    }

    /// `SetFirmEndpoint`: every check before the confirm, so a refused endpoint asks nothing and
    /// a page cannot write its own words into the dialog: `signin.policy_unreadable` while
    /// policy is unreadable for a firm plan, `settings.locked` for a plan policy sets, and
    /// `command.invalid` for an address the settings crate would refuse. Then the confirm, then
    /// the endpoint alone written, the plan's provider and variables kept.
    async fn set_endpoint(
        &self,
        agent: &AgentId,
        info: &AgentInfo,
        url: Option<String>,
    ) -> Result<(), HostError> {
        self.refuse_blocked(agent)?;
        if info.plan_locked {
            return Err(HostError::new(
                ErrorCode::SettingsLocked,
                format!("the firm's policy sets plans.{agent}"),
            ));
        }
        let url = url.as_deref().map(checked_endpoint).transpose()?;
        if url.is_some() {
            self.refuse_leaving_reused(agent)?;
        }
        let name = info.name.clone();
        self.ask(Ask::Endpoint {
            name,
            url: url.clone(),
        })
        .await?;
        let firm = url.is_some() || info.provider;
        self.host.set_endpoint(agent.clone(), url).await?;
        self.compose.set_firm(agent, firm);
        if firm {
            self.end_sign_in(agent);
        }
        Ok(())
    }

    /// `RetryAgentInstall`: `command.invalid` unless `agent`'s install is `Failed`, `Waiting` or
    /// `Paused`, since it asks nothing; a missing agent installs through `InstallAgent`'s confirm.
    fn retry(&self, agent: &AgentId, use_system_proxy: bool) -> Result<(), HostError> {
        let restartable = matches!(
            self.agents.state(agent),
            Some(InstallState::Failed(_) | InstallState::Waiting { .. } | InstallState::Paused)
        );
        if !restartable {
            return Err(invalid(format!(
                "{agent}'s install is not failed, waiting or paused; installing asks first"
            )));
        }
        self.agents.retry(agent, use_system_proxy);
        Ok(())
    }

    /// What a key write runs once memory has changed, still in the keychain's order: tells
    /// [`Compose`] whether `agent` has a key.
    fn key_trigger(&self, agent: &AgentId) -> impl FnOnce(bool) + Send + 'static {
        let compose = Arc::clone(&self.compose);
        let agent = agent.clone();
        move |stored| compose.set_key(&agent, stored)
    }

    /// Asks the lawyer `ask`; a decline is `transport.unauthorized`.
    async fn ask(&self, ask: Ask) -> Result<(), HostError> {
        if self.confirm.confirm(ask).await {
            Ok(())
        } else {
            Err(HostError::new(
                ErrorCode::TransportUnauthorized,
                "the lawyer declined",
            ))
        }
    }

    /// The refusal of a sign-in start for `agent`: an unreadable policy, else what bars it
    /// (`admit::sign_in_bar`: a firm policy's endpoint, a key, an endpoint the lawyer set).
    fn refuse_start(&self, agent: &AgentId) -> Result<(), HostError> {
        self.refuse_blocked(agent)?;
        match self.compose.sign_in_bar(agent) {
            Some(bar) => Err(sign_in_barred(agent, bar)),
            None => Ok(()),
        }
    }

    /// Ends `agent`'s sign-in, if one runs or is reserved, because a key or an endpoint took
    /// over: always `Agents::cancel_sign_in`, the `CancelSignIn` path, since S18 reserves a
    /// sign-in's job at once but reports its first state only from the worker, so
    /// `signin_state` cannot tell a reserved job from none. [`Compose::expect_end`] goes
    /// first, so the end reaches the windows as the new line and not as a failure; S18's
    /// `NotRunning` (nothing to end) undoes it and is no error. Then the starting mark goes
    /// (`SetupEvents::end_start`): the cancelled job may never report.
    fn end_sign_in(&self, agent: &AgentId) {
        let _gate = lock(&self.signin_gate);
        self.compose.expect_end(agent);
        match self.agents.cancel_sign_in(agent) {
            Ok(()) => tracing::info!(%agent, "ending a sign-in: a key or an endpoint took over"),
            Err(SigninError::NotRunning(_)) => self.compose.no_end(agent),
            Err(error) => {
                self.compose.no_end(agent);
                tracing::warn!(%agent, %error, "a sign-in a key or an endpoint took over runs on");
            }
        }
        self.events.end_start(agent);
    }

    /// `agent`'s new S18 job (a sign-in, a sign-out or a search) replaced any it had, so an
    /// end a key or an endpoint was waiting for ([`Compose::expect_end`]) will not come: the
    /// replaced job's report is dropped, and the new job's failure is shown as one.
    fn replaced(&self, agent: &AgentId) {
        self.compose.no_end(agent);
    }

    /// Whether a firm policy sets `agent`'s endpoint or provider: told to [`Compose`], which
    /// then bars its sign-in with `signin.firm_managed`, and a running sign-in ended. Read at
    /// start; desktop has no live policy feed yet, so a policy that arrives later takes force
    /// at the next start, through here too.
    pub(crate) fn set_firm_policy(&self, agent: &AgentId, managed: bool) {
        self.compose.set_firm_policy(agent, managed);
        if managed {
            self.end_sign_in(agent);
        }
    }

    /// `signin.policy_unreadable` while `agent`'s sign-in is refused for an unreadable policy.
    fn refuse_blocked(&self, agent: &AgentId) -> Result<(), HostError> {
        if self.compose.blocked(agent) {
            return Err(HostError::new(
                ErrorCode::SigninPolicyUnreadable,
                format!("the firm's policy for {agent} cannot be read"),
            ));
        }
        Ok(())
    }

    /// Whether `agent`'s login is the lawyer's own, reused.
    fn reused(&self, agent: &AgentId) -> bool {
        matches!(
            self.agents.signin_state(agent),
            Some(SigninState::SignedIn {
                source: Source::Reused,
                ..
            })
        )
    }

    /// `internal` while `agent`'s login is reused: leaving it for a key or an endpoint runs the
    /// leave order, piece B's.
    fn refuse_leaving_reused(&self, agent: &AgentId) -> Result<(), HostError> {
        if self.reused(agent) {
            return Err(leave_order_missing());
        }
        Ok(())
    }

    /// Follows `Agents`' events until they end: installs out as `AgentInstallProgress` (and the
    /// entry written on `Ready`), sign-ins through [`Compose`], `OpenUrl` opened, notes logged.
    pub(crate) async fn pump(&self, source: impl Stream<Item = AgentsEvent>) {
        let mut source = std::pin::pin!(source);
        let mut first_run = FirstRun::new(self.known.keys().cloned());
        let mut checked = BTreeSet::new();
        while let Some(event) = source.next().await {
            match event {
                AgentsEvent::Install { agent, state } => {
                    self.install_event(&agent, &state, &mut checked).await;
                    if first_run.saw(&agent, &state)
                        && let Ok(first) = AgentId::new(FIRST_RUN_AGENT)
                        && self.known.contains_key(&first)
                    {
                        tracing::info!(agent = %first, "first run: installing");
                        self.agents.install(&first);
                    }
                }
                AgentsEvent::Signin { agent, state } => self.compose.signin(&agent, state),
                AgentsEvent::OpenUrl { agent, url } => {
                    if let Err(error) = open_url_unconfirmed(&*self.opener, &url) {
                        tracing::warn!(%agent, code = error.code().as_str(), "a sign-in page did not open");
                    }
                }
                AgentsEvent::SigninNote { agent, note } => events::note(&agent, note.code()),
                _ => {}
            }
        }
    }

    /// One install state: sent to the windows; on `Ready` the entry written and the status
    /// checked (once per agent); an agent with no sign-in state yet shown signed out.
    async fn install_event(
        &self,
        agent: &AgentId,
        state: &InstallState,
        checked: &mut BTreeSet<AgentId>,
    ) {
        let Some(info) = self.known.get(agent) else {
            return;
        };
        if let Some(progress) = progress(state, &info.hosts) {
            self.events.publish(SetupEvent::AgentInstallProgress {
                agent: agent.clone(),
                state: progress,
            });
        }
        if matches!(state, InstallState::Ready { .. }) {
            self.write_entry(agent, info).await;
            if checked.insert(agent.clone())
                && let Err(error) = self.agents.check_status(agent)
            {
                tracing::warn!(%agent, %error, "no status check");
            }
        } else if self.compose.s18(agent).is_none() && self.agents.signin_state(agent).is_none() {
            self.compose
                .signin(agent, SigninState::SignedOut { found: None });
        }
    }

    /// Writes `agent`'s `agents.<id>` entry from its install, logging the outcome by code.
    async fn write_entry(&self, agent: &AgentId, info: &AgentInfo) {
        let Some(installed) = self.agents.installed(agent) else {
            return;
        };
        let source = match self.compose.s18(agent) {
            Some(SigninState::SignedIn {
                source: Source::Reused,
                ..
            }) => SignInSource::Reused,
            _ => SignInSource::Own,
        };
        let Some(launch) = launch(agent, &installed, &info.home, source) else {
            tracing::warn!(%agent, "the install's paths are not UTF-8: no entry written");
            return;
        };
        match self.host.set_agent_launch(agent.clone(), launch).await {
            Ok(EntryWrite::Written) => tracing::info!(%agent, "agent entry written"),
            Ok(EntryWrite::LeftAlone) => {
                tracing::info!(%agent, "agent entry left alone: it is the lawyer's or the firm's");
            }
            Err(error) => {
                tracing::warn!(%agent, code = error.code().as_str(), "agent entry not written");
            }
        }
    }
}

/// Which first install states have come in: first run is decided once every agent has one.
struct FirstRun {
    waiting: BTreeSet<AgentId>,
    /// Whether an agent's first state was `Ready` or `Managed`.
    any_present: bool,
    decided: bool,
}

impl FirstRun {
    fn new(agents: impl IntoIterator<Item = AgentId>) -> Self {
        Self {
            waiting: agents.into_iter().collect(),
            any_present: false,
            decided: false,
        }
    }

    /// Notes `agent`'s `state`; `true` once, when the last first state came in and none was
    /// `Ready` or `Managed`: first run.
    fn saw(&mut self, agent: &AgentId, state: &InstallState) -> bool {
        if self.decided || !self.waiting.remove(agent) {
            return false;
        }
        self.any_present |= matches!(state, InstallState::Ready { .. } | InstallState::Managed);
        if !self.waiting.is_empty() {
            return false;
        }
        self.decided = true;
        !self.any_present
    }
}

/// The plan login `StartSignIn` asks for: Claude's Claude.ai plan or, with `console`, its
/// Console account; Codex's browser sign-in (`console` is Claude's alone).
fn plan_method(agent: &AgentId, console: bool) -> Result<Method, HostError> {
    match (AgentKind::of(agent), console) {
        (Some(AgentKind::Claude), false) => Ok(Method::ClaudeAi),
        (Some(AgentKind::Claude), true) => Ok(Method::Console),
        (Some(AgentKind::Codex), false) => Ok(Method::Browser),
        _ => Err(invalid(format!("{agent} has no such sign-in"))),
    }
}

/// The endpoint `url` as the confirm shows it and the plan stores it: an address the settings
/// crate takes (`web_address_ok`: ASCII only, no space, line break, control character, `?`,
/// `#`, `@` or backslash) that parses as an `https` URL with a host and no user name or
/// password, else `command.invalid` before any confirm. The message never repeats it.
fn checked_endpoint(url: &str) -> Result<String, HostError> {
    if !clauseo_settings::web_address_ok(url) {
        return Err(invalid(
            "the endpoint must be an https address of plain characters, with no query or fragment",
        ));
    }
    let parsed = url::Url::parse(url).map_err(|_error| invalid("the endpoint is not a URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(invalid(
            "the endpoint must be an https address with no user name or password",
        ));
    }
    Ok(url.to_owned())
}

/// The answer to a refused S18 call: `command.invalid`, except a missing home (`internal`).
/// The text names the agent, never a code or an email.
fn signin_error(error: &SigninError) -> HostError {
    let code = match error {
        SigninError::NoHome(_) => ErrorCode::Internal,
        _ => ErrorCode::CommandInvalid,
    };
    HostError::new(code, error.to_string())
}

/// The answer to what needs 0048's leave order (R20 P5b piece B adds it); the words name no
/// plan stage.
fn leave_order_missing() -> HostError {
    HostError::new(
        ErrorCode::Internal,
        "leaving a login you already had is not available yet in this version of Clauseo",
    )
}

/// A `command.invalid` failure with `message`.
fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

mod entry;
mod seam;
#[cfg(test)]
mod tests;
