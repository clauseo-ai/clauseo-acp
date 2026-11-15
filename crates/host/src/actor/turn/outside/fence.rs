//! The fence between matters, read on the actor: which other matters a launch of a thread
//! fences and which it is invited into (`agent::matter_fence`), and the key a live agent's
//! launch took of them, which a change to the matter list or to the thread's invites makes
//! stale (`idle::fence`, which stops the agent once it is idle so its next prompt relaunches it
//! with the new rules).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use clauseo_protocol::{
    HeaderEntry, MatterId, Notice, NoticeKind, NoticeLevel, StreamId, ThreadId,
};
use serde_json::Value;

use super::super::super::Actor;
use super::super::launch_checks::RunFolders;
use crate::agent::matter_fence::{FenceKey, MatterFence, Other};
use crate::log_file::Cause;

/// The thread's header: the loaded stream's, else the one kept when it was unloaded.
fn header(actor: &Actor, thread: ThreadId) -> Option<&HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// The other matters the lawyer let `thread` read, from its header, oldest first.
pub(crate) fn read_matters(actor: &Actor, thread: ThreadId) -> Vec<MatterId> {
    header(actor, thread).map_or_else(Vec::new, |header| header.read_matters.clone())
}

/// The other matters a launch of `thread` (in matter `own`) fences, not yet resolved: every
/// matter the host knows but `own`, forgotten ones too (their folders are still the lawyer's
/// other clients' files), each invited when the thread's header names it and it is on the
/// list (a forgotten matter's invite waits for it to come back).
pub(crate) fn matter_fence(actor: &Actor, thread: ThreadId, own: MatterId) -> MatterFence {
    let invited = read_matters(actor, thread);
    let listed = actor.matters.values().map(|entry| (entry, true));
    let forgotten = actor.forgotten.values().map(|entry| (entry, false));
    let others = listed
        .chain(forgotten)
        .filter(|(entry, _)| entry.row.id != own)
        .map(|(entry, on_list)| {
            let row = &entry.row;
            let folder = PathBuf::from(&row.folder_path);
            let invited = on_list && invited.contains(&row.id);
            Other::new(row.id, row.name.clone(), folder, invited)
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    fence.key = key_of(actor, thread, matters_hash(actor));
    fence
}

/// The key a launch of `thread` takes now (`FenceKey`); `matters` is [`matters_hash`], made
/// once for all threads.
pub(crate) fn key_of(actor: &Actor, thread: ThreadId, matters: u64) -> FenceKey {
    let mut hasher = DefaultHasher::new();
    let mut invited = read_matters(actor, thread);
    invited.sort();
    invited.hash(&mut hasher);
    FenceKey {
        matters,
        invited: hasher.finish(),
    }
}

/// Every matter's id and folder the host knows, hashed, listed and forgotten apart.
pub(crate) fn matters_hash(actor: &Actor) -> u64 {
    let mut hasher = DefaultHasher::new();
    for (list, entries) in [(0_u8, &actor.matters), (1, &actor.forgotten)] {
        list.hash(&mut hasher);
        for (id, entry) in entries {
            id.hash(&mut hasher);
            entry.row.folder_path.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// The instructions part for the matters `run`'s conversation may read
/// (`clauseo_profile::instructions::matter_reads_part`): each one's name and folder as stored;
/// for Codex (`inside` false) not one inside the own matter, which stays fenced for it
/// (`agent::codex_fence::Session::other_matters`).
pub(crate) fn reads_part(run: &RunFolders, inside: bool) -> String {
    let fence = &run.data_deny.matters;
    let within = |other: &crate::agent::matter_fence::Other| {
        other
            .spellings
            .iter()
            .any(|spelling| fence.inside_own.iter().any(|(p, _)| p == spelling))
    };
    let reads: Vec<(&str, &str)> = fence
        .others
        .iter()
        .filter(|other| other.invited && (inside || !within(other)))
        .filter_map(|other| Some((other.name.as_str(), other.folder.to_str()?)))
        .collect();
    clauseo_profile::instructions::matter_reads_part(&reads)
}

/// `part` appended to the instructions `text`, when it is a string and `part` is not empty.
pub(crate) fn append_part(text: &mut Value, part: &str) {
    if let Value::String(text) = text
        && !part.is_empty()
    {
        text.push_str(part);
    }
}

/// The other matters' rule (`{other_matters}`) for a Codex launch: on Windows (`windows`),
/// where nothing fences Codex from them, the name-free rule that knows a matter by its
/// `.clauseo` folder (`clauseo_profile::instructions::other_matters_unfenced`); elsewhere its
/// sandbox fences them and it gets the fenced rule, as Claude does everywhere.
pub(crate) fn codex_rule(windows: bool) -> &'static str {
    use clauseo_profile::instructions::{OTHER_MATTERS, other_matters_unfenced};
    if windows {
        other_matters_unfenced()
    } else {
        OTHER_MATTERS.trim()
    }
}

/// What a Claude session's `meta` gets from the fence after its instructions are filled: the
/// invited matters' part appended to them, and the lawyer told of the matters the launch could
/// not fence and those its Windows shell rules left out ([`tell_unfenced`]), and that Claude's
/// sandboxed commands can't open the matter when a fenced matter lies inside it or `limited`
/// (a deny widened to a folder above, `agent::loads::sandbox::deny_form`). Claude's alone:
/// Codex's instructions and unfenced line go with its keys (`profile_loads::launch_keys`), and
/// on Windows it has no shell rules to report.
pub(crate) fn after_meta(
    actor: &mut Actor,
    thread: ThreadId,
    run: &RunFolders,
    meta: &mut serde_json::Map<String, Value>,
    limited: bool,
) {
    let reads = reads_part(run, true);
    let prompt = meta.get_mut("systemPrompt");
    if let Some(text) = prompt.and_then(|prompt| prompt.get_mut("append")) {
        append_part(text, &reads);
    }
    let mut unfenced = run.data_deny.matters.unfenced.clone();
    let shell = crate::agent::loads::profile::unfenced_matters(meta, &run.data_deny, cfg!(windows));
    let new: Vec<MatterId> = shell
        .into_iter()
        .filter(|m| !unfenced.contains(m))
        .collect();
    unfenced.extend(new);
    tell_unfenced(actor, thread, unfenced);
    // Claude on macOS and Linux alone has the sandbox (its short temp folder was made).
    let sandboxed = run.claude_tmp.is_some();
    let shut = run.data_deny.matters.own_shut || limited;
    tell_limited(actor, thread, shut && sandboxed);
}

/// The words the limited notice falls back to, for a view that does not know its kind.
const LIMITED_TEXT: &str = "Claude's commands can't reach some folders right now; it can still read and \
    edit this matter's files.";

/// Tells the lawyer, once while it lasts, that Claude's sandboxed commands can't reach some
/// folders: the matter (`MatterFence::own_shut`: the sandbox re-opens none of it), or a folder
/// above another matter's that its deny was widened to (`agent::loads::sandbox::deny_form`).
/// Any other agent's launch passes `false`: it ends what Claude's last launch told.
pub(crate) fn tell_limited(actor: &mut Actor, thread: ThreadId, limited: bool) {
    let Some(state) = actor.threads.get_mut(&thread).map(|entry| &mut entry.turn) else {
        return;
    };
    if std::mem::replace(&mut state.told_once.shell_limited, limited) || !limited {
        return;
    }
    tracing::warn!(%thread, "Claude's commands are kept out of some folders this session");
    let notice = Notice {
        level: NoticeLevel::Info,
        text: LIMITED_TEXT.to_owned(),
        kind: Some(NoticeKind::ShellLimited),
    };
    if let Err(error) = super::super::updates::commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the limited notice was not logged");
    }
}

/// The words the unfenced notice falls back to, for a view that does not know its kind.
const UNFENCED_TEXT: &str = "Clauseo couldn't fully keep the AI out of some other matters.";

/// Tells the lawyer the other matters a launch left unfenced (`MatterFence::unfenced`, and for
/// Claude on Windows those its shell rules could not fit,
/// `agent::loads::profile::unfenced_matters`): one notice naming at most five of them,
/// when that set differs from the one the thread was last told in this host run. Counts only in
/// the log.
pub(crate) fn tell_unfenced(actor: &mut Actor, thread: ThreadId, unfenced: Vec<MatterId>) {
    let Some(state) = actor.threads.get_mut(&thread).map(|entry| &mut entry.turn) else {
        return;
    };
    if state.told_once.unfenced == unfenced {
        return;
    }
    state.told_once.unfenced.clone_from(&unfenced);
    if unfenced.is_empty() {
        return;
    }
    tracing::warn!(%thread, matters = unfenced.len(), "other matters left out of the shell rules");
    let more = u32::try_from(unfenced.len().saturating_sub(5)).unwrap_or(u32::MAX);
    let notice = Notice {
        level: NoticeLevel::Warning,
        text: UNFENCED_TEXT.to_owned(),
        kind: Some(NoticeKind::MattersUnfenced {
            matters: unfenced.into_iter().take(5).collect(),
            more,
        }),
    };
    if let Err(error) = super::super::updates::commit(actor, thread, [notice.into()], Vec::new()) {
        tracing::warn!(%thread, error = %Cause(&error), "the unfenced notice was not logged");
    }
}
