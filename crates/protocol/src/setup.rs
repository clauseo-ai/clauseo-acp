//! Sign-in and installs on the wire (decision record 0048): the [`SetupCommand`]s a window sends
//! through desktop's `setup` Tauri command, and the [`SetupEvent`]s desktop sends back in its
//! `setup` stream arm. Neither is a `HostCommand` or a `HostEvent`: desktop answers both itself,
//! with the `agents` crate, which this crate cannot depend on. The states here mirror `agents`'
//! `InstallState` and `SigninState`, and desktop maps each into its wire form.
//!
//! A pasted sign-in code and an API key cross as a [`Secret`], which decodes but never encodes.
//! No type here holds a key's or a variable's value.

mod install;
mod secret;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use self::install::{
    InstallLog, InstallProgress, MAX_INSTALL_LOG_LINE_BYTES, MAX_INSTALL_LOG_LINES,
};
pub use self::secret::{MAX_SECRET_BYTES, Secret};
use crate::{AgentId, ErrorCode, PlanLabel, Timestamp};

/// What a window asks of an agent's install or sign-in. JSON: `{"start_sign_in": {"agent":
/// "claude-acp", "console": false}}`; unknown fields and unknown commands are refused. Decode
/// only: it may hold a [`Secret`], so it has no `Serialize`, and no `Clone`.
///
/// Which window may send which command is desktop's allow-list (0048, "Allow-lists").
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SetupCommand {
    /// Install the agent, after a native confirm naming it and its size; another agent's
    /// download pauses.
    InstallAgent {
        /// The agent.
        agent: AgentId,
    },
    /// Start a failed or paused install again.
    RetryAgentInstall {
        /// The agent.
        agent: AgentId,
        /// Whether to go through the OS's proxy this time; missing means `false`.
        #[serde(default)]
        use_system_proxy: bool,
    },
    /// Sign in with the agent's plan login. A sign-in already running for the agent is joined,
    /// not restarted.
    StartSignIn {
        /// The agent.
        agent: AgentId,
        /// Claude only: sign in with an Anthropic Console account instead of a Claude.ai plan;
        /// missing means `false`.
        #[serde(default)]
        console: bool,
    },
    /// Stop the sign-in that is running.
    CancelSignIn {
        /// The agent.
        agent: AgentId,
    },
    /// Give the running sign-in the code the lawyer pasted (Claude's paste route).
    PasteSignInCode {
        /// The agent.
        agent: AgentId,
        /// The pasted code: never stored, logged or shown.
        code: Secret,
    },
    /// Switch Codex's sign-in to its device code.
    UseDeviceCode {
        /// The agent.
        agent: AgentId,
    },
    /// Open the sign-in page again ("Didn't open?"); nothing happens when no page is known.
    OpenSignInPage {
        /// The agent.
        agent: AgentId,
    },
    /// Sign Clauseo's own login out. Refused with `command.invalid` for a reused login.
    SignOut {
        /// The agent.
        agent: AgentId,
    },
    /// "Use it": reuse the login [`SignIn::LoginFound`] showed.
    UseFoundLogin {
        /// The agent.
        agent: AgentId,
    },
    /// "Sign in with a different account": leave the found or reused login and sign in.
    UseDifferentAccount {
        /// The agent.
        agent: AgentId,
    },
    /// "Stop using it here": stop reusing the lawyer's own login; it stays signed in.
    StopUsingLogin {
        /// The agent.
        agent: AgentId,
    },
    /// Look again for a login the lawyer already has, turning reuse back on. Its end is one
    /// `SignInChanged`, sent even when the state is unchanged: `LoginFound`, `SignedOut` with
    /// `found` (`none_found` for nothing), `SignedIn`, or `Failed` (`signin.failed`) when
    /// the search could not tell.
    FindLogin {
        /// The agent.
        agent: AgentId,
    },
    /// Store an API key for the agent in the OS keychain.
    SetAgentKey {
        /// The agent.
        agent: AgentId,
        /// The key: kept only in the keychain.
        key: Secret,
    },
    /// Remove the agent's API key from the keychain.
    ClearAgentKey {
        /// The agent.
        agent: AgentId,
    },
    /// Set or clear the agent's firm endpoint (`plans.<id>.endpoint`), after a native confirm.
    /// The settings crate checks the URL.
    SetFirmEndpoint {
        /// The agent.
        agent: AgentId,
        /// The endpoint, an `https` URL; missing or `null` clears it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
}

impl SetupCommand {
    /// The agent the command is about.
    pub const fn agent(&self) -> &AgentId {
        match self {
            Self::InstallAgent { agent }
            | Self::RetryAgentInstall { agent, .. }
            | Self::StartSignIn { agent, .. }
            | Self::CancelSignIn { agent }
            | Self::PasteSignInCode { agent, .. }
            | Self::UseDeviceCode { agent }
            | Self::OpenSignInPage { agent }
            | Self::SignOut { agent }
            | Self::UseFoundLogin { agent }
            | Self::UseDifferentAccount { agent }
            | Self::StopUsingLogin { agent }
            | Self::FindLogin { agent }
            | Self::SetAgentKey { agent, .. }
            | Self::ClearAgentKey { agent }
            | Self::SetFirmEndpoint { agent, .. } => agent,
        }
    }

    /// Which command it is, without its fields: what desktop's allow-lists name.
    pub const fn action(&self) -> SetupAction {
        match self {
            Self::InstallAgent { .. } => SetupAction::InstallAgent,
            Self::RetryAgentInstall { .. } => SetupAction::RetryAgentInstall,
            Self::StartSignIn { .. } => SetupAction::StartSignIn,
            Self::CancelSignIn { .. } => SetupAction::CancelSignIn,
            Self::PasteSignInCode { .. } => SetupAction::PasteSignInCode,
            Self::UseDeviceCode { .. } => SetupAction::UseDeviceCode,
            Self::OpenSignInPage { .. } => SetupAction::OpenSignInPage,
            Self::SignOut { .. } => SetupAction::SignOut,
            Self::UseFoundLogin { .. } => SetupAction::UseFoundLogin,
            Self::UseDifferentAccount { .. } => SetupAction::UseDifferentAccount,
            Self::StopUsingLogin { .. } => SetupAction::StopUsingLogin,
            Self::FindLogin { .. } => SetupAction::FindLogin,
            Self::SetAgentKey { .. } => SetupAction::SetAgentKey,
            Self::ClearAgentKey { .. } => SetupAction::ClearAgentKey,
            Self::SetFirmEndpoint { .. } => SetupAction::SetFirmEndpoint,
        }
    }
}

/// A [`SetupCommand`]'s variant without its fields, for desktop's allow-lists. Never on the
/// wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SetupAction {
    /// [`SetupCommand::InstallAgent`].
    InstallAgent,
    /// [`SetupCommand::RetryAgentInstall`].
    RetryAgentInstall,
    /// [`SetupCommand::StartSignIn`].
    StartSignIn,
    /// [`SetupCommand::CancelSignIn`].
    CancelSignIn,
    /// [`SetupCommand::PasteSignInCode`].
    PasteSignInCode,
    /// [`SetupCommand::UseDeviceCode`].
    UseDeviceCode,
    /// [`SetupCommand::OpenSignInPage`].
    OpenSignInPage,
    /// [`SetupCommand::SignOut`].
    SignOut,
    /// [`SetupCommand::UseFoundLogin`].
    UseFoundLogin,
    /// [`SetupCommand::UseDifferentAccount`].
    UseDifferentAccount,
    /// [`SetupCommand::StopUsingLogin`].
    StopUsingLogin,
    /// [`SetupCommand::FindLogin`].
    FindLogin,
    /// [`SetupCommand::SetAgentKey`].
    SetAgentKey,
    /// [`SetupCommand::ClearAgentKey`].
    ClearAgentKey,
    /// [`SetupCommand::SetFirmEndpoint`].
    SetFirmEndpoint,
}

impl SetupAction {
    /// Every action, in declaration order.
    pub const ALL: [Self; 15] = [
        Self::InstallAgent,
        Self::RetryAgentInstall,
        Self::StartSignIn,
        Self::CancelSignIn,
        Self::PasteSignInCode,
        Self::UseDeviceCode,
        Self::OpenSignInPage,
        Self::SignOut,
        Self::UseFoundLogin,
        Self::UseDifferentAccount,
        Self::StopUsingLogin,
        Self::FindLogin,
        Self::SetAgentKey,
        Self::ClearAgentKey,
        Self::SetFirmEndpoint,
    ];
}

/// What desktop's `setup` stream arm carries. A new subscription first gets `SetupInfo`, then
/// each agent's install and sign-in state, before any newer event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SetupEvent {
    /// What this computer offers.
    SetupInfo {
        /// Whether the OS keychain can hold an API key: false where Linux has no Secret
        /// Service, and the key screens are hidden.
        keys_available: bool,
    },
    /// An agent's install state changed, or its download made progress.
    AgentInstallProgress {
        /// The agent.
        agent: AgentId,
        /// Its new state.
        state: InstallProgress,
    },
    /// An agent's sign-in state changed.
    SignInChanged {
        /// The agent.
        agent: AgentId,
        /// Its new state.
        state: SignIn,
    },
}

/// Where an agent's sign-in is: `agents`' `SigninState`, with how the agent is signed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SignIn {
    /// Not signed in.
    SignedOut {
        /// What the search for a login saw (a login not reusable, none, or reuse off), or why
        /// reuse stopped; missing when nothing was looked for (the agent not installed, a key
        /// or an endpoint that took over, a sign-out).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        found: Option<FoundLogin>,
    },
    /// Waiting for the install to finish or the network to return; the sign-in then starts by
    /// itself.
    Queued,
    /// The login run is going and the vendor's page should be open.
    Waiting {
        /// When the run started.
        since: Timestamp,
        /// Whether to offer "Didn't open?" (true after 30 seconds).
        nudge: bool,
    },
    /// Codex's device code: the lawyer opens `url` and types `code` before `expires_at`.
    DeviceCode {
        /// The device page, on one of the agent's sign-in hosts.
        url: String,
        /// The one-time code to type, such as `ABCD-1234`; shown, so not a [`Secret`].
        code: String,
        /// When the code stops working.
        expires_at: Timestamp,
    },
    /// A login the lawyer already has, not used until they choose "Use it".
    LoginFound {
        /// Whose login it is.
        from: LoginFrom,
        /// The account, from the agent's own status output, when it names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        email: Option<String>,
    },
    /// Signed in.
    SignedIn {
        /// The account's email, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        email: Option<String>,
        /// The plan in words, such as "Claude Max", when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<PlanLabel>,
        /// How: a plan login, an API key or the firm's endpoint.
        method: SignInMethod,
        /// Whose login: the lawyer's own, reused, or Clauseo's.
        source: SignInSource,
        /// When the sign-in was confirmed.
        at: Timestamp,
    },
    /// The sign-in stopped: a `signin.*` code, `agent.auth_required`, or
    /// `signin.policy_unreadable`.
    Failed {
        /// Why.
        code: ErrorCode,
        /// When.
        at: Timestamp,
    },
    /// Clauseo is leaving a login it may no longer use (a split Codex link, or a reused login
    /// the agent's own CLI ended) and the leave did not finish: it tries again by itself, and
    /// nothing of the agent launches meanwhile. Ends when a try succeeds (the state after it)
    /// or a new sign-in arrives; it has no failed end.
    Switching {
        /// When the first try did not finish; a later try never moves it.
        since: Timestamp,
        /// Whether it has gone on for 2 minutes or more.
        stuck: bool,
    },
}

/// What the search for a login saw, or why reuse stopped: [`SignIn::SignedOut`]'s `found`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FoundLogin {
    /// A Claude login set in the lawyer's own Claude settings (not a `claude.ai` one).
    ClaudeSettings,
    /// Windows with the sandbox set up, where Claude's login is not reused.
    ClaudeWindowsSandbox,
    /// A Codex login held in the OS keyring, not in a file.
    CodexKeyring,
    /// Reuse stopped: a refresh failed twice.
    ReusedFailed,
    /// Reuse stopped: the agent's own CLI is signed out.
    ReusedSignedOut,
    /// Reuse stopped: the agent's own CLI is signed in to another account.
    ReusedChanged,
    /// Reuse stopped: Codex's shared login file was replaced, so it is no longer shared.
    CodexLinkSplit,
    /// The search for a login ran and found none of the lawyer's on this computer.
    NoneFound,
    /// Reuse is off here ("Sign in with a different account", or "Stop using it here"), so
    /// nothing was looked for; [`SetupCommand::FindLogin`] turns it back on.
    ReuseOff,
}

impl FoundLogin {
    /// Every value, in declaration order.
    pub const ALL: [Self; 9] = [
        Self::ClaudeSettings,
        Self::ClaudeWindowsSandbox,
        Self::CodexKeyring,
        Self::ReusedFailed,
        Self::ReusedSignedOut,
        Self::ReusedChanged,
        Self::CodexLinkSplit,
        Self::NoneFound,
        Self::ReuseOff,
    ];
}

/// Whose login [`SignIn::LoginFound`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum LoginFrom {
    /// The lawyer's Claude Code.
    ClaudeCode,
    /// The lawyer's Codex CLI.
    Codex,
}

impl LoginFrom {
    /// Every value, in declaration order.
    pub const ALL: [Self; 2] = [Self::ClaudeCode, Self::Codex];
}

/// How a signed-in agent is signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SignInMethod {
    /// The vendor's plan login (a Claude.ai plan, a Console account, a `ChatGPT` plan).
    Plan,
    /// An API key from the keychain.
    ApiKey,
    /// The firm's endpoint or provider (`plans.<id>`).
    Firm,
}

impl SignInMethod {
    /// Every value, in declaration order.
    pub const ALL: [Self; 3] = [Self::Plan, Self::ApiKey, Self::Firm];
}

/// Where a signed-in login came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SignInSource {
    /// The lawyer's own CLI login on this computer, reused after "Use it". Clauseo never signs
    /// it out.
    Reused,
    /// Clauseo's own sign-in, a key or an endpoint.
    Own,
}

impl SignInSource {
    /// Every value, in declaration order.
    pub const ALL: [Self; 2] = [Self::Reused, Self::Own];
}
