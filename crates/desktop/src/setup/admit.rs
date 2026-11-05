//! Which window may send which setup action (decision record 0048, "Allow-lists", and its
//! rulings): one table, [`windows`], keyed on the window labels `main` (`MAIN_WINDOW`: UD1's
//! first run and the room's sign-in prompts) and `settings` (`SETTINGS_WINDOW`: UD2's Plans,
//! which runs its own sign-in too). Any other window may send none. The refusals' codes are
//! here too, in one place: `transport.unauthorized` for a window or an action not allowed (as
//! 0048 says; `dispatch` and `query` refuse a window with `command.invalid`),
//! `settings.key_stored` for a key `main` may not replace, and the three refusals of a sign-in
//! while the agent is on another service ([`sign_in_bar`]).

use clauseo_protocol::{AgentId, ErrorCode, HostError, SetupAction};

use super::KeyWrite;
use crate::settings_window::SETTINGS_WINDOW;

/// Whether `main` may send `SetAgentKey`, storing a key only while the agent has none (the
/// orchestrator's ruling on 0048). Replacing or clearing a key stays the settings window's.
pub(super) const MAIN_SETS_FIRST_KEY: bool = true;

/// The windows an action is allowed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Windows {
    /// `main` and `settings`.
    Both,
    /// `settings` alone.
    Settings,
    /// `settings`, and `main` while [`MAIN_SETS_FIRST_KEY`] holds, for a first key only.
    SettingsOrFirstKey,
}

/// The allow-list: one arm per action and no catch-all, so a new action does not compile
/// until it is ruled on.
const fn windows(action: SetupAction) -> Windows {
    match action {
        SetupAction::InstallAgent
        | SetupAction::RetryAgentInstall
        | SetupAction::StartSignIn
        | SetupAction::CancelSignIn
        | SetupAction::PasteSignInCode
        | SetupAction::UseDeviceCode
        | SetupAction::OpenSignInPage
        | SetupAction::UseFoundLogin
        | SetupAction::UseDifferentAccount => Windows::Both,
        SetupAction::SetAgentKey => Windows::SettingsOrFirstKey,
        SetupAction::SignOut
        | SetupAction::ClearAgentKey
        | SetupAction::SetFirmEndpoint
        | SetupAction::FindLogin
        | SetupAction::StopUsingLogin => Windows::Settings,
    }
}

/// Whether the window `label` may send any setup action, and so follows the setup events.
pub(crate) fn may_setup(label: &str) -> bool {
    label == crate::MAIN_WINDOW || label == SETTINGS_WINDOW
}

/// Whether the window `label` may send `action`, with `main`'s first key allowed or not: how a
/// key it sends may be stored, or the refusal.
pub(super) fn admit(
    main_sets_first_key: bool,
    label: &str,
    action: SetupAction,
) -> Result<KeyWrite, HostError> {
    let main = label == crate::MAIN_WINDOW;
    let allowed = match windows(action) {
        Windows::Both => may_setup(label),
        Windows::Settings => label == SETTINGS_WINDOW,
        Windows::SettingsOrFirstKey => label == SETTINGS_WINDOW || (main && main_sets_first_key),
    };
    if !allowed {
        return Err(refused(label, action));
    }
    if main && action == SetupAction::SetAgentKey {
        Ok(KeyWrite::OnlyIfAbsent)
    } else {
        Ok(KeyWrite::Replace)
    }
}

/// The refusal of `action` from the window `label`.
fn refused(label: &str, action: SetupAction) -> HostError {
    HostError::new(
        ErrorCode::TransportUnauthorized,
        format!("window {label} may not send {action:?}"),
    )
}

/// The refusal of a key for `agent` that may not replace the one stored: what a backend
/// answers a [`KeyWrite::OnlyIfAbsent`] write when a key is stored. No key text.
pub(crate) fn key_stored(agent: &AgentId) -> HostError {
    HostError::new(
        ErrorCode::SettingsKeyStored,
        format!("a key is already stored for {agent}; it is changed in Settings"),
    )
}

/// Why an agent's sign-in may not start from any window (the orchestrator's ruling on 0048):
/// it is on a service a plan login would not reach. No silent switch, and no sign-in a key or
/// an endpoint then hides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignInBar {
    /// A firm endpoint set by policy: `signin.firm_managed`.
    FirmManaged,
    /// An API key stored: `signin.key_in_use`.
    KeyInUse,
    /// An endpoint set in Settings: `signin.endpoint_in_use`.
    EndpointInUse,
}

/// The bar on a sign-in, if any, from what holds: the firm's policy wins, then a key, then an
/// endpoint the lawyer set.
pub(crate) fn sign_in_bar(firm_policy: bool, key: bool, endpoint: bool) -> Option<SignInBar> {
    if firm_policy {
        Some(SignInBar::FirmManaged)
    } else if key {
        Some(SignInBar::KeyInUse)
    } else if endpoint {
        Some(SignInBar::EndpointInUse)
    } else {
        None
    }
}

/// The refusal of `agent`'s sign-in for `bar`: the condition only, never a key, an address or
/// a host.
pub(crate) fn sign_in_barred(agent: &AgentId, bar: SignInBar) -> HostError {
    let (code, why) = match bar {
        SignInBar::FirmManaged => (
            ErrorCode::SigninFirmManaged,
            "a firm endpoint is set by policy",
        ),
        SignInBar::KeyInUse => (ErrorCode::SigninKeyInUse, "an API key is stored"),
        SignInBar::EndpointInUse => (
            ErrorCode::SigninEndpointInUse,
            "an endpoint is set in Settings",
        ),
    };
    HostError::new(code, format!("{agent}'s sign-in is refused: {why}"))
}
