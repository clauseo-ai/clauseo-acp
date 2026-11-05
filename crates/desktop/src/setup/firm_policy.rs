//! The `firm-policy` marker (decision record 0048, "Security keys and their safe values"): when
//! a firm policy has set an agent's `plans.<id>.endpoint` or `.provider`, desktop writes
//! `<data_dir>/agents/<id>/firm-policy` (it holds no secret). While a policy source cannot be
//! read, `plans.*` take their safe value (no plan at all), and an agent with that marker is
//! refused sign-in with `signin.policy_unreadable`, never sent to the plan login.
//!
//! It fails closed: with policy unreadable, a marker whose presence cannot be told (any error
//! but "not found") counts as there. A marker that cannot be written is logged; the next start
//! writes it again while the policy still sets the plan.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;
use clauseo_settings::{Effective, Files};

/// The marker's name, in `<data_dir>/agents/<id>/`.
pub(crate) const MARKER: &str = "firm-policy";

/// What the marker says. No secret: only that the firm set the plan.
const MARKER_TEXT: &str = "A firm policy set this agent's plan (plans.<id>). While a policy \
                           source cannot be read, Clauseo refuses this agent's sign-in.\n";

/// Where `agent`'s marker is.
pub(crate) fn marker(data_dir: &Path, agent: &AgentId) -> PathBuf {
    data_dir.join("agents").join(agent.as_str()).join(MARKER)
}

/// Brings each of `agents`' markers up to date with `effective`, and gives the agents whose
/// sign-in is refused: with every policy source read, the marker is written for each agent
/// whose plan policy sets with an endpoint or a provider, and none is refused; with a source
/// unreadable, nothing is written, and each agent whose marker is there (or may be) is refused.
pub(crate) fn firm_policy<'a>(
    effective: &Effective,
    files: &dyn Files,
    agents: impl IntoIterator<Item = &'a AgentId>,
) -> BTreeSet<AgentId> {
    let data_dir = effective.data_dir();
    let unreadable = !effective.policy().errors().is_empty();
    let mut blocked = BTreeSet::new();
    for agent in agents {
        let path = marker(data_dir, agent);
        if unreadable {
            if marker_may_be_there(&path) {
                blocked.insert(agent.clone());
            }
        } else if firm_set(effective, agent)
            && let Err(error) = write_marker(files, &path)
        {
            tracing::warn!(%agent, kind = ?error.kind(), "the firm-policy marker was not written");
        }
    }
    blocked
}

/// Whether policy sets `agent`'s plan with an endpoint or a provider.
fn firm_set(effective: &Effective, agent: &AgentId) -> bool {
    effective.is_locked(&format!("plans.{agent}"))
        && effective
            .plan(agent.as_str())
            .is_some_and(|plan| plan.endpoint.is_some() || plan.provider.is_some())
}

/// Whether the marker at `path` is there, or may be: only "not found" says it is not.
fn marker_may_be_there(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// Writes the marker at `path` through `files` (which makes its folder), unless it is there.
fn write_marker(files: &dyn Files, path: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Ok(());
    }
    files.write_atomic(path, MARKER_TEXT.as_bytes())
}

#[cfg(test)]
mod tests;
