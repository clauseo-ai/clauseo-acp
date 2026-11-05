//! Papers an undo removed that come back. A sync client can put a file back after Clauseo's undo
//! deleted it (`OneDrive` on macOS, about 3 s later, as a cloud-only placeholder), or later
//! still. After an undo that deleted files, the host looks for them twice, [`FIRST_CHECK`] and
//! [`LAST_CHECK`] after the commit logging the undo is durable, on the host clock: each check
//! stats the files not found back yet on the matter's thread (`Matter::still_gone`, which never
//! reads or downloads). The files the undo's own checkpoint already found back
//! (`Undone::resurrected`) are reported at the first check.
//!
//! A check that finds files back logs one `Notice` (`NoticeKind::FilesCameBack`, its text a
//! count only) and takes them out of the agent's undo note if it has not gone out yet. Each file
//! is reported once, and never deleted again. Shutdown disarms every check; a check of a thread
//! or matter the host no longer has does nothing. The log has counts only, never paths.

use std::time::Duration;

use clauseo_protocol::{MatterId, MatterPath, Notice, NoticeKind, NoticeLevel, StreamId, ThreadId};
use futures::future::AbortHandle;

use super::super::input::{Input, MatterTag};
use super::super::{Actor, HostPhase};
use super::out::Out;
use super::{Then, TurnInput, prompt, state};
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{MatterJob, MatterReply};

/// The first check, after the undo is logged: a sync client's quick put-back is seen by then.
pub(crate) const FIRST_CHECK: Duration = Duration::from_secs(10);
/// The last check, after the undo is logged.
pub(crate) const LAST_CHECK: Duration = Duration::from_secs(60);
/// The most files a notice names; the rest are counted in `more`.
const NAMED: usize = 5;

/// One thread's watches, one per undo that deleted files.
#[derive(Debug, Default)]
pub(crate) struct Watches {
    next: u64,
    watches: Vec<Watch>,
}

#[derive(Debug)]
struct Watch {
    id: u64,
    matter: MatterId,
    /// Deleted and not found back yet.
    gone: Vec<MatterPath>,
    /// Found back by the undo's own checkpoint; reported at the first check.
    back: Vec<MatterPath>,
    timers: [AbortHandle; 2],
}

/// A check that is due, or whose stat the matter's thread answers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Check {
    thread: ThreadId,
    watch: u64,
    /// The last of the two checks.
    last: bool,
}

/// The undo of `thread` that deleted `deleted` in `matter` is logged: arms its two checks.
/// `resurrected`, the files its checkpoint already found back, are reported at the first.
pub(super) fn watch(
    actor: &mut Actor,
    thread: ThreadId,
    matter: MatterId,
    deleted: Vec<MatterPath>,
    resurrected: &[MatterPath],
) {
    if deleted.is_empty() || !matches!(actor.phase, HostPhase::Serving) {
        return;
    }
    let Some(entry) = actor.threads.get(&thread) else {
        return;
    };
    let id = entry.came_back.next;
    let (back, gone): (Vec<MatterPath>, Vec<MatterPath>) = deleted
        .into_iter()
        .partition(|path| resurrected.contains(path));
    let arm = |actor: &Actor, delay, last| {
        let check = Check {
            thread,
            watch: id,
            last,
        };
        actor.post_after(delay, Input::Turn(TurnInput::CameBack(check)))
    };
    let timers = [arm(actor, FIRST_CHECK, false), arm(actor, LAST_CHECK, true)];
    if let Some(entry) = actor.threads.get_mut(&thread) {
        entry.came_back.next += 1;
        entry.came_back.watches.push(Watch {
            id,
            matter,
            gone,
            back,
            timers,
        });
    }
}

/// A check is due: stats the files not found back yet, or reports at once when none are left
/// to stat.
pub(super) fn on_due(actor: &mut Actor, check: Check) {
    if !matches!(actor.phase, HostPhase::Serving) {
        return;
    }
    let Some(watch) = find(actor, check) else {
        return;
    };
    if watch.gone.is_empty() {
        found(actor, check, &[]);
        return;
    }
    let (matter, paths) = (watch.matter, watch.gone.clone());
    let sent = actor
        .matter_worker(matter)
        .and_then(|worker| worker.send(MatterJob::StillGone { paths }, MatterTag::CameBack(check)));
    match sent {
        Ok(()) => actor.work.matter_jobs += 1,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "papers an undo removed were not looked for");
            found(actor, check, &[]);
        }
    }
}

/// The matter's thread answered a check's stat.
pub(crate) fn on_reply(actor: &mut Actor, check: Check, reply: MatterReply) {
    let MatterReply::StillGone(result) = reply else {
        tracing::error!(
            reply = reply.name(),
            "a came-back check answered with another job's reply"
        );
        found(actor, check, &[]);
        return;
    };
    let back = result.unwrap_or_else(|error| {
        tracing::warn!(error = %Cause(&error), "papers an undo removed were not looked for");
        Vec::new()
    });
    found(actor, check, &back);
}

/// What `check` found back: with the files the undo's checkpoint found, each reported once in
/// one notice, and taken out of the agent's note. The last check, or one that leaves nothing
/// to look for, ends the watch.
fn found(actor: &mut Actor, check: Check, back: &[MatterPath]) {
    let Some(watches) = actor
        .threads
        .get_mut(&check.thread)
        .map(|entry| &mut entry.came_back)
    else {
        return;
    };
    let Some(at) = watches.watches.iter().position(|w| w.id == check.watch) else {
        return;
    };
    let mut reported = Vec::new();
    {
        let Some(watch) = watches.watches.get_mut(at) else {
            return;
        };
        reported.append(&mut watch.back);
        watch.gone.retain(|path| {
            let now_back = back.contains(path);
            if now_back {
                reported.push(path.clone());
            }
            !now_back
        });
    }
    let done = check.last || watches.watches.get(at).is_none_or(|w| w.gone.is_empty());
    if done {
        let watch = watches.watches.remove(at);
        for timer in &watch.timers {
            timer.abort();
        }
    }
    if reported.is_empty() || !matches!(actor.phase, HostPhase::Serving) {
        return;
    }
    reported.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    reported.dedup();
    tracing::info!(
        files = reported.len(),
        "papers an undo removed came back; left in place"
    );
    trim_note(actor, check.thread, &reported);
    log_notice(actor, check.thread, reported);
}

/// Takes `reported` out of the thread's undo note, if the next prompt has not taken it yet.
fn trim_note(actor: &mut Actor, thread: ThreadId, reported: &[MatterPath]) {
    prompt::ensure_note(actor, thread);
    if let Some(note) = state(actor, thread).and_then(|state| state.note.as_mut()) {
        note.retain(|path| !reported.contains(path));
    }
}

/// The notice of `reported`, logged on the thread once its stream is loaded.
fn log_notice(actor: &mut Actor, thread: ThreadId, reported: Vec<MatterPath>) {
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| {
                let mut out = Out::new(actor, thread, None)?;
                out.push(actor, notice(reported))?;
                out.send(actor, Then::Nothing)
            });
            if let Err(error) = logged {
                tracing::warn!(%thread, error = %Cause(&error), "a came-back notice was not logged");
            }
            actor.release(stream);
        }),
    );
}

/// The notice for `files` (sorted): at most [`NAMED`] named, the rest counted; its text, for a
/// view that does not know the kind, gives the count only.
pub(super) fn notice(mut files: Vec<MatterPath>) -> Notice {
    let n = files.len();
    let more = u32::try_from(n.saturating_sub(NAMED)).unwrap_or(u32::MAX);
    files.truncate(NAMED);
    let text = if n == 1 {
        "1 paper came back after the undo. Clauseo didn't remove it again.".to_owned()
    } else {
        format!("{n} papers came back after the undo. Clauseo didn't remove them again.")
    };
    Notice {
        level: NoticeLevel::Info,
        text,
        kind: Some(NoticeKind::FilesCameBack { files, more }),
    }
}

/// Shutdown: every check is disarmed and forgotten.
pub(super) fn shutdown(actor: &mut Actor) {
    for entry in actor.threads.values_mut() {
        for watch in entry.came_back.watches.drain(..) {
            for timer in &watch.timers {
                timer.abort();
            }
        }
    }
}

fn find(actor: &Actor, check: Check) -> Option<&Watch> {
    let entry = actor.threads.get(&check.thread)?;
    entry.came_back.watches.iter().find(|w| w.id == check.watch)
}
