//! The guard before a Codex login over Clauseo's own login: `codex login` revokes and deletes the
//! login it finds before it starts, so such a login needs the lawyer's confirmed switch.

use std::path::Path;

use clauseo_protocol::AgentId;

use crate::signin::SigninError;
use crate::signin::codex::written::Stamp;
use crate::signin::env::Vendor;
use crate::signin::marker::{self, LoginSource};

/// Whether a Codex login may replace Clauseo's own login: only once the lawyer confirmed the
/// switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Switch {
    /// Refused while the login would replace Clauseo's own (`SigninError::OwnLoginPresent`).
    Refuse,
    /// The lawyer confirmed the switch.
    Confirmed,
}

/// Clauseo's own Codex login as a stat of its file saw it (file id, size, modified time; the
/// file is never opened), so a caller that keeps the lawyer's yes to replacing it can tell
/// whether the login there later is still the one the yes was for. Only compared, never read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnLogin(Stamp);

impl OwnLogin {
    /// Test support: a login told apart from others by `n` alone, for a fake `Agents`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(n: u64) -> Self {
        Self(Stamp::for_tests(n))
    }
}

/// The Clauseo-own login a Codex `login` for `agent` now would replace: Codex, its
/// `login-source` (under `data_dir`) says `own`, and a stat finds the login file in `home`.
/// Files only, never the shown state, which is `None` or `Queued` at start-up.
pub(super) fn own_login(
    data_dir: &Path,
    agent: &AgentId,
    vendor: Vendor,
    home: Option<&Path>,
) -> Option<OwnLogin> {
    let home = home?;
    if vendor != Vendor::Codex || marker::read_source(data_dir, agent) != Some(LoginSource::Own) {
        return None;
    }
    Stamp::of(home).map(OwnLogin)
}

/// `OwnLoginPresent` when the login `replaces` Clauseo's own and `switch` is not the yes.
pub(super) fn guard(switch: Switch, replaces: bool, agent: &AgentId) -> Result<(), SigninError> {
    if switch == Switch::Refuse && replaces {
        return Err(SigninError::OwnLoginPresent(agent.clone()));
    }
    Ok(())
}
