//! The guard before a Codex login over Clauseo's own login: `codex login` revokes and deletes the
//! login it finds before it starts, so such a login needs the lawyer's confirmed switch.

use std::path::Path;

use clauseo_protocol::AgentId;

use crate::signin::SigninError;
use crate::signin::env::Vendor;
use crate::signin::marker::{self, LoginSource};
use crate::signin::reuse;

/// Whether a Codex login may replace Clauseo's own login: only once the lawyer confirmed the
/// switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Switch {
    /// Refused while the login would replace Clauseo's own (`SigninError::OwnLoginPresent`).
    Refuse,
    /// The lawyer confirmed the switch.
    Confirmed,
}

/// Whether a Codex `login` for `agent` now would replace Clauseo's own login: Codex, its
/// `login-source` (under `data_dir`) says `own`, and a stat finds the login file in `home`.
/// Files only, never the shown state, which is `None` or `Queued` at start-up.
pub(super) fn replaces_own_login(
    data_dir: &Path,
    agent: &AgentId,
    vendor: Vendor,
    home: Option<&Path>,
) -> bool {
    let Some(home) = home else {
        return false;
    };
    vendor == Vendor::Codex
        && marker::read_source(data_dir, agent) == Some(LoginSource::Own)
        && reuse::home_login_present(home)
}

/// `OwnLoginPresent` when the login `replaces` Clauseo's own and `switch` is not the yes.
pub(super) fn guard(switch: Switch, replaces: bool, agent: &AgentId) -> Result<(), SigninError> {
    if switch == Switch::Refuse && replaces {
        return Err(SigninError::OwnLoginPresent(agent.clone()));
    }
    Ok(())
}
