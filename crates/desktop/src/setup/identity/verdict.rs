//! [`Verdict`]: what the identity check knows of an agent's login's account, and the refusal
//! each one puts on its launches (the state table in decision record 0048).

use clauseo_protocol::ErrorCode;

/// What desktop knows of an agent's login's account now. Watched means a change of a login
/// file's stamps checks it. While a sign-in through Clauseo is pending, a change of the stamps
/// it took makes a verdict that refuses nothing [`Verdict::Due`] until its record lands (which
/// holds for those stamps); its bound passing first gives [`Verdict::Unknown`] and a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Verdict {
    /// Not signed in (S18's resting state is not `SignedIn`), or a first-run status named no
    /// account. Refuses nothing. Watched; with an account recorded, also checked again on the
    /// macOS Keychain timer.
    #[default]
    Idle,
    /// A check is due: every agent at start until its startup check answers (first run
    /// included), a key login that ended, or a stale macOS Keychain check, which waits for a
    /// launch to ask. Refuses `agent.identity_checking`.
    Due,
    /// A check runs. Refuses `agent.identity_checking`; a change during it shows at the next
    /// look, against the stamps taken before its read.
    Checking,
    /// The status named the recorded account. Refuses nothing. Watched, and on the macOS
    /// Keychain timer.
    Same,
    /// Signed out through Clauseo (`SignOut`), the account still recorded, so another account
    /// coming back reads as changed: S18's resting state after it, or a status naming no
    /// account then. Refuses nothing (a launch meets the agent's own sign-in refusal,
    /// `agent.auth_required`). Watched, so a login file coming back checks; on the macOS
    /// Keychain timer; no other timer.
    SignedOut,
    /// The status failed, timed out or named no account while one is recorded (not after a
    /// sign-out through Clauseo); an account with no record after a launch; the record cannot
    /// be read; a pending sign-in's bound passed after its files changed; or the followers
    /// panicked. Refuses `agent.identity_unknown`. Watched; checked again every
    /// [`UNKNOWN_AGAIN`](super::UNKNOWN_AGAIN) only after a status that failed or timed out,
    /// or a panic.
    Unknown,
    /// The status named another account: the agents were stopped. Refuses
    /// `signin.account_changed` until a sign-in through Clauseo records again; not watched.
    Changed,
    /// Not checked: the launch env carries a credential that replaces the shared login (a stored
    /// API key, or a Claude provider), so a swapped file cannot redirect its launches; or the
    /// agent is firm-managed, with no Clauseo install to read a status from (a documented gap:
    /// it still launches on Clauseo's login home). Not checked, not
    /// watched, and no refusal while it holds (`setup/refusal.rs` filters this one's). Once it
    /// ends the login is what launches use again: due a check, as [`Verdict::Due`], first.
    KeyLogin,
}

impl Verdict {
    /// The launch refusal this verdict puts on the agent, if any.
    pub(crate) fn refusal(self) -> Option<ErrorCode> {
        match self {
            Verdict::Idle | Verdict::Same | Verdict::SignedOut => None,
            Verdict::Due | Verdict::Checking | Verdict::KeyLogin => {
                Some(ErrorCode::AgentIdentityChecking)
            }
            Verdict::Unknown => Some(ErrorCode::AgentIdentityUnknown),
            Verdict::Changed => Some(ErrorCode::SigninAccountChanged),
        }
    }

    /// The log's fixed word for it.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Verdict::Idle => "idle",
            Verdict::Due => "due",
            Verdict::Checking => "checking",
            Verdict::Same => "same",
            Verdict::SignedOut => "signed_out",
            Verdict::Unknown => "unknown",
            Verdict::Changed => "changed",
            Verdict::KeyLogin => "key_login",
        }
    }
}
