//! Codex's pre-card check of a permission request's places (`agent/data_fence/other_matters.rs`):
//! the places its tool call named, and the matters' folders it is compared with.

use std::path::PathBuf;

use clauseo_protocol::{AgentKind, ThreadId};

use super::super::super::Actor;
use super::super::{current, matter_of};
use super::PermissionAsk;
use crate::agent::data_fence;

/// The places the request's tool call named (its locations and diffs), as absolute paths: those
/// inside the matter joined to its folder, the others as the agent sent them.
pub(super) fn codex_call_places(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
) -> Vec<PathBuf> {
    let Some(places) =
        current(actor, thread).map(|current| current.tool_paths.places(&ask.acp_tool_call_id))
    else {
        return Vec::new();
    };
    let folder = matter_of(actor, thread)
        .ok()
        .and_then(|matter| actor.matters.get(&matter))
        .map(|entry| PathBuf::from(&entry.row.folder_path));
    let inside = places
        .inside
        .iter()
        .filter_map(|path| Some(folder.as_ref()?.join(path.as_str())));
    inside
        .chain(places.outside.iter().map(PathBuf::from))
        .collect()
}

/// Whether the thread's agent is Codex.
pub(super) fn is_codex(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent))
        == Some(AgentKind::Codex)
}

/// This matter's folder, every other matter's the host knows now, as stored, and the command's
/// words (`raw_input`), for Codex's pre-card check (`agent/data_fence/other_matters.rs`), with
/// the agent launch's `cache` of their folders resolved; none when the thread's matter is not
/// known.
pub(super) fn other_matters(
    actor: &Actor,
    thread: ThreadId,
    raw_input: Option<&serde_json::Value>,
    cache: &data_fence::other_matters::Cache,
) -> Option<data_fence::other_matters::OtherMatters> {
    let matter = matter_of(actor, thread).ok()?;
    let own = PathBuf::from(&actor.matters.get(&matter)?.row.folder_path);
    let others = actor
        .matters
        .iter()
        .filter(|(id, _)| **id != matter)
        .map(|(_, entry)| PathBuf::from(&entry.row.folder_path))
        .collect();
    let args = data_fence::input::command_args(raw_input, cfg!(windows));
    Some(data_fence::other_matters::OtherMatters {
        own,
        others,
        args,
        cache: cache.clone(),
    })
}
