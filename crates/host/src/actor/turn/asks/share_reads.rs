//! Claude Code's own ask before a Read, Grep or Glob of a network path, answered by the host,
//! once and with nothing logged, when the read stays inside the matter folder stored by its
//! network address (`agent/network_matter/reads.rs`). The tool call itself still shows; changes,
//! other servers and shares, other spellings and anything outside the matter go on as before.

use std::path::PathBuf;

use clauseo_acp::PendingKey;
use clauseo_protocol::{AgentChoiceId, AgentKind, PermissionKind, ThreadId};

use super::super::super::Actor;
use super::super::spawn;
use super::super::updates::agent;
use super::PermissionAsk;
use crate::agent::network_matter::admin_policy::AdminTier;
use crate::agent::network_matter::{by_address, reads};
use crate::log_file::Cause;

/// The `allow_once` option to answer this request with, when the host answers it itself:
/// - the thread's agent is Claude, its launch found the matter on the network and the matter
///   is stored by its address (`network_matter::by_address`, Windows only);
/// - its launch found no administrator policy tier (`Agent::admin_tier`): a firm's
///   `permissions.ask` rule asks just as Claude Code's own check does, and the request does not
///   say which one raised it;
/// - the request offers an option of kind `allow_once`;
/// - `tool` (the tool call's name) and the request's own `raw_input` read only inside the
///   matter as stored ([`reads::own_share_read`]);
/// - and Windows' link policy follows no link on a share to another server or to this
///   computer ([`reads::links_followed`]).
pub(super) fn own_read_to_allow(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
    tool: Option<&str>,
    raw_input: Option<&serde_json::Value>,
) -> Option<AgentChoiceId> {
    let row = &actor.check_thread(thread).ok()?.row;
    if AgentKind::of(&row.agent) != Some(AgentKind::Claude) {
        return None;
    }
    let matter = row.matter;
    let stored = PathBuf::from(&actor.check_matter(matter).ok()?.row.folder_path);
    let agent = agent(actor, thread)?;
    let tier = agent.admin_tier != AdminTier::Absent;
    if tier || !by_address(agent.folder_check, &stored, cfg!(windows)) {
        return None;
    }
    let option = ask
        .options
        .iter()
        .find(|option| option.kind == PermissionKind::AllowOnce)?;
    let option = AgentChoiceId::new(&option.option_id).ok()?;
    if !reads::own_share_read(tool, raw_input, &stored, &agent.folder) || links_followed() {
        return None;
    }
    Some(option)
}

/// Whether Windows' link policy may follow a link on a share (read now); no value off Windows,
/// where no matter is stored by its address.
fn links_followed() -> bool {
    #[cfg(windows)]
    let values = crate::network_paths::link_policy();
    #[cfg(not(windows))]
    let values = Vec::new();
    reads::links_followed(&values)
}

/// Answers the request with `option` (`allow_once`), with nothing logged on the thread.
/// `tracing` gets the tool's name, never its path.
pub(super) fn allow_own_read(
    actor: &mut Actor,
    thread: ThreadId,
    key: PendingKey,
    tool: Option<&str>,
    option: AgentChoiceId,
) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let client = agent.link.client.clone();
    let tool = tool.unwrap_or_default();
    tracing::info!(%thread, tool, "allowed a read inside a matter stored by its network address");
    spawn(actor, async move {
        if let Err(error) = client.answer_permission(key, option).await {
            tracing::warn!(error = %Cause(&error), "an own-share read's approval did not reach the agent");
        }
    });
}
