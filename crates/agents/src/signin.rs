//! Sign-in (S18): the states an agent's sign-in reports, its failure codes, the pasted code, and
//! the two pieces every sign-in, status and detection run is built from: the child's whole
//! environment (`env`) and the runner (`run`).
//!
//! The flows themselves (Claude's `auth login` with the `BROWSER` shim, Codex's `login` and
//! `--device-auth`, detection and reuse) are built on these, in `signin/claude.rs`,
//! `signin/codex.rs`, `signin/shim.rs` and `signin/reuse.rs`. Each flow:
//! - builds its environment with `env::run_env`, from [`crate::SigninConfig::base_env`], and
//!   for Codex adds `env::codex_store_args` to its arguments;
//! - starts `<node> <script> args…` with `run::run`, through the `Launcher` the caller gave
//!   [`crate::Agents::start`]. That launcher must be made with `SpawnOptions::clear_env`, since
//!   the built environment is the child's whole environment (see the crate docs, Invariants);
//! - reads status output with the parsers in `crate::status`;
//! - compares `auth.json` names with [`crate::same_file`], never by opening either.
//!
//! Clauseo never opens or reads a credential, and nothing here names a credential file: the
//! test `tests/credential_names.rs` fails if any file under `src` does outside a comment.

use std::time::SystemTime;

use zeroize::Zeroizing;

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "S18's foundation: the Claude and Codex sign-in flows that call it come next"
    )
)]
pub(crate) mod env;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "S18's foundation: the Claude and Codex sign-in flows that call it come next"
    )
)]
pub(crate) mod run;

/// Where an agent's sign-in is. R20 maps each state to the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SigninState {
    /// Not signed in. `found` is a login seen but not reusable, or why reuse stopped.
    SignedOut {
        /// What was found, if anything.
        found: Option<Found>,
    },
    /// Sign-in was pressed before the install was ready, or while offline: the run starts by
    /// itself once both hold.
    Queued,
    /// The login run is going and the vendor's page should be open. `nudge` turns true at
    /// 30 s ("Didn't open?").
    Waiting {
        /// When the run started.
        since: SystemTime,
        /// Whether to offer "Didn't open?".
        nudge: bool,
    },
    /// Codex's device-code run: the lawyer opens `url` and types `code` before `expires_at`.
    DeviceCode {
        /// The device page, whose host is one of the snapshot's sign-in hosts.
        url: String,
        /// The one-time code, `XXXX-XXXX`.
        code: String,
        /// When the code stops working (15 minutes after it was given).
        expires_at: SystemTime,
    },
    /// A login the lawyer already has, reusable once they choose "Use it". Detection never
    /// starts reuse.
    LoginFound {
        /// Whose login it is.
        from: LoginFrom,
        /// The account, from the agent's own status output; never from a token file.
        email: Option<String>,
    },
    /// Signed in. `plan` is the raw plan string; R20 turns it into words.
    SignedIn {
        /// The account's email, when the status reports it.
        email: Option<String>,
        /// The raw plan string (`max`, `plus`, …), when the status reports it.
        plan: Option<String>,
        /// Whether the login is the lawyer's own, reused, or Clauseo's.
        source: Source,
        /// When the sign-in was confirmed.
        at: SystemTime,
    },
    /// The sign-in stopped with `code`.
    Failed {
        /// Why.
        code: SigninCode,
        /// When.
        at: SystemTime,
    },
}

/// A login seen but not reusable, or why reuse stopped: `SignedOut`'s `found`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Found {
    /// A Claude login made in the lawyer's `~/.claude/settings.json` (an `authMethod` other
    /// than `claude.ai`): not reusable.
    ClaudeSettings,
    /// Windows with tier B set up: Claude is not reused there.
    ClaudeWindowsSandbox,
    /// A Codex login held in the OS keyring or Windows secrets, not in a file: not reusable.
    CodexKeyring,
    /// Reuse stopped: a refresh failed twice.
    ReusedFailed,
    /// Reuse stopped: the CLI's own status is signed out (for Codex, its file is gone).
    ReusedSignedOut,
    /// Reuse stopped: Claude's status names another email than the accepted one.
    ReusedChanged,
    /// Reuse stopped: the two names of Codex's login are no longer one file.
    CodexLinkSplit,
}

/// Whose login [`SigninState::LoginFound`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LoginFrom {
    /// The lawyer's Claude Code.
    ClaudeCode,
    /// The lawyer's Codex CLI.
    Codex,
}

/// Where a signed-in login came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    /// The lawyer's own CLI login, reused after "Use it". S18 never signs it out.
    Reused,
    /// Clauseo's own sign-in, in Clauseo's folder.
    Own,
}

/// Why a sign-in stopped. Each is one error code, which R20 adds to `ErrorCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SigninCode {
    /// The lawyer refused on the vendor's page, or the run's output says it was cancelled.
    Cancelled,
    /// The link or device code expired: 10 minutes for Claude, the code's 15 for Codex.
    LinkExpired,
    /// The account has no plan, where a status field says so.
    NoPlan,
    /// Codex's device-code sign-in is turned off for the account.
    DeviceCodeOff,
    /// Port 1455, Codex's sign-in callback, is taken.
    PortBusy,
    /// The run ended before the sign-in, for any other reason.
    Failed,
}

impl SigninCode {
    /// The error code: `signin.cancelled`, `signin.link_expired`, `signin.no_plan`,
    /// `signin.device_code_off`, `signin.port_busy` or `signin.failed`.
    pub fn code(self) -> &'static str {
        match self {
            SigninCode::Cancelled => "signin.cancelled",
            SigninCode::LinkExpired => "signin.link_expired",
            SigninCode::NoPlan => "signin.no_plan",
            SigninCode::DeviceCodeOff => "signin.device_code_off",
            SigninCode::PortBusy => "signin.port_busy",
            SigninCode::Failed => "signin.failed",
        }
    }
}

/// The code the lawyer pastes for Claude's paste route. It goes with a newline to the login's
/// stdin and is then dropped, zeroed; it is never stored, logged or shown. `Debug` prints
/// `<code>`, and there is no `Display` and no `Clone`.
pub struct PasteCode(Zeroizing<String>);

impl PasteCode {
    /// Wraps the pasted text.
    pub fn new(code: Zeroizing<String>) -> PasteCode {
        PasteCode(code)
    }

    /// What the login's stdin gets: the code without spaces at either end and without any
    /// control character (so a paste cannot add a second line), then `\n`. Zeroed when dropped.
    pub(crate) fn line(&self) -> Zeroizing<Vec<u8>> {
        let mut line = Zeroizing::new(Vec::with_capacity(self.0.len().saturating_add(1)));
        line.extend(
            self.0
                .trim()
                .bytes()
                .filter(|byte| !byte.is_ascii_control()),
        );
        line.push(b'\n');
        line
    }
}

impl From<String> for PasteCode {
    fn from(code: String) -> PasteCode {
        PasteCode(Zeroizing::new(code))
    }
}

impl std::fmt::Debug for PasteCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<code>")
    }
}

#[cfg(test)]
mod tests;
