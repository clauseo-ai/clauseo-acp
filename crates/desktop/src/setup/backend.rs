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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_agents::{
    AgentsEvent, InstallState, Installed, Method, PasteCode, SigninError, SigninState, Source,
};
use clauseo_protocol::{
    AgentId, AgentKind, CommandOutcome, Count, ErrorCode, HostError, InstallLog, InstallProgress,
    SetupCommand, SetupEvent, SignInSource,
};
use clauseo_settings::{AgentLaunch, EntryWrite};
use futures::{Stream, StreamExt as _};

use super::compose::Compose;
use super::confirm::{Ask, SetupConfirm};
use super::host_calls::SetupHost;
use super::keys::KeyCache;
use super::{KeyWrite, SetupBackend, SetupEvents, events};
use crate::dialogs::BoxFuture;
use crate::open::{Opener, open_url_unconfirmed};

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
                self.refuse_blocked(&agent)?;
                let method = plan_method(&agent, console)?;
                agents
                    .sign_in(&agent, method)
                    .map_err(|error| signin_error(&error))?;
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
                self.refuse_blocked(&agent)?;
                agents
                    .sign_in(&agent, Method::DeviceCode)
                    .map_err(|error| signin_error(&error))?;
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
            }
            SetupCommand::UseFoundLogin { .. } => {
                self.refuse_blocked(&agent)?;
                agents
                    .use_found_login(&agent)
                    .map_err(|error| signin_error(&error))?;
            }
            SetupCommand::UseDifferentAccount { .. } => {
                self.refuse_blocked(&agent)?;
                agents
                    .use_different_account(&agent)
                    .map_err(|error| signin_error(&error))?;
            }
            SetupCommand::StopUsingLogin { .. } => return Err(leave_order_missing()),
            SetupCommand::FindLogin { .. } => agents
                .find_login(&agent)
                .map_err(|error| signin_error(&error))?,
            SetupCommand::SetAgentKey { key, .. } => {
                self.refuse_blocked(&agent)?;
                self.refuse_leaving_reused(&agent)?;
                let then = self.key_trigger(&agent);
                self.keys.set(&agent, key, key_write, then).await?;
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

/// The `agents.<id>` entry for `installed` (decision record 0048, "Installed becomes
/// launchable"): `command` the Node, `args` the adapter's script, and the env: for Claude
/// `ANTHROPIC_API_KEY` empty, `CLAUDE_CONFIG_DIR` the home and `CLAUDE_SECURESTORAGE_CONFIG_DIR`
/// empty for a `Reused` login or the home for `Own`; for Codex `CODEX_API_KEY` empty and, on
/// macOS and Linux, `HOME` the home. `None` when a path is not UTF-8 (settings hold text).
pub(crate) fn launch(
    agent: &AgentId,
    installed: &Installed,
    home: &Path,
    source: SignInSource,
) -> Option<AgentLaunch> {
    let command = installed.node.to_str()?.to_owned();
    let script = installed.script.to_str()?.to_owned();
    let home = home.to_str()?.to_owned();
    let mut env = BTreeMap::new();
    match AgentKind::of(agent) {
        Some(AgentKind::Claude) => {
            env.insert("ANTHROPIC_API_KEY".to_owned(), String::new());
            env.insert("CLAUDE_CONFIG_DIR".to_owned(), home.clone());
            let storage = match source {
                SignInSource::Reused => String::new(),
                SignInSource::Own => home,
            };
            env.insert("CLAUDE_SECURESTORAGE_CONFIG_DIR".to_owned(), storage);
        }
        Some(AgentKind::Codex) => {
            env.insert("CODEX_API_KEY".to_owned(), String::new());
            if cfg!(not(windows)) {
                env.insert("HOME".to_owned(), home);
            }
        }
        None => {}
    }
    Some(AgentLaunch {
        command,
        args: vec![script],
        env,
    })
}

/// `state` on the wire, with `hosts` for a waiting or failed install; `None` for a state this
/// version does not know.
pub(crate) fn progress(state: &InstallState, hosts: &[String]) -> Option<InstallProgress> {
    Some(match state {
        InstallState::Missing { download_bytes } => InstallProgress::Missing {
            download_bytes: Count::clamped(*download_bytes),
        },
        InstallState::Waiting { reason, log } => InstallProgress::Waiting {
            code: ErrorCode::from_str_lossy(reason.code()),
            hosts: hosts.to_vec(),
            log: InstallLog::new(log.lines().iter().cloned()),
        },
        InstallState::Downloading { received, total } => InstallProgress::Downloading {
            received: Count::clamped(*received),
            total: Count::clamped(*total),
        },
        InstallState::Installing => InstallProgress::Installing,
        InstallState::Ready { version } => InstallProgress::Ready {
            version: version.clone(),
        },
        InstallState::Failed(failure) => InstallProgress::Failed {
            code: ErrorCode::from_str_lossy(failure.kind.code()),
            need_mb: failure.need_mb.map(Count::clamped),
            free_mb: failure.free_mb.map(Count::clamped),
            hosts: hosts.to_vec(),
            log: InstallLog::new(failure.log.lines().iter().cloned()),
        },
        InstallState::Paused => InstallProgress::Paused,
        InstallState::Managed => InstallProgress::Managed,
        _ => return None,
    })
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

mod seam;
#[cfg(test)]
mod tests;
