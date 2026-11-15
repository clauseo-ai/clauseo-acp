//! The browser guard's queued fallback deliveries (`sources/browser/fallback/queue.rs`): a file
//! its last sweep saved in Clauseo's own downloads folder for the matter after the last tool
//! result, so no agent line told it and no notice showed. Read when a session of the matter
//! ends ([`take`], off the actor in the run folder's removal; a reaper still sweeping then is
//! read the next time) and when the host starts serving ([`queued_at_start`]: a host that
//! crashed saw no session end). Each file is shown once, as the agent line's would be:
//! `NoticeKind::DownloadOutsideMatter`, in the thread whose session queued it.

use std::path::Path;

use clauseo_protocol::{MatterId, StreamId, ThreadId};

use super::super::super::super::Actor;
use super::super::super::out::Out;
use super::super::super::{Then, state};
use crate::error::Error;
use crate::log_file::Cause;
use crate::sources::browser::{fallback_notice, fallback_queue};

/// The fallback deliveries queued for the matter whose notes folder is `notes`; the queue
/// files go. File system calls: off the actor.
pub(super) fn take(notes: &Path) -> Vec<fallback_queue::Queued> {
    fallback_queue::take(&crate::agent::loads::browser::fallback_downloads(notes))
}

/// At the host's start: the queues every known matter's fallback folder holds.
pub(crate) fn queued_at_start(actor: &mut Actor) {
    let data = actor.effective.data_dir().to_owned();
    let matters: Vec<MatterId> = actor.matters.keys().copied().collect();
    for matter in matters {
        let folder = crate::agent::loads::browser::matter_fallback_downloads(&data, matter);
        let queued = fallback_queue::take(&folder);
        tell(actor, matter, None, queued);
    }
}

/// Shows each of `queued` once in its thread: the one its session's id names when that is a
/// thread of `matter`, else `fallback` (the ending session's); one with neither is dropped.
pub(super) fn tell(
    actor: &mut Actor,
    matter: MatterId,
    fallback: Option<ThreadId>,
    queued: Vec<fallback_queue::Queued>,
) {
    for file in queued {
        let named = file.thread.filter(|thread| {
            actor
                .threads
                .get(thread)
                .is_some_and(|entry| entry.row.matter == matter)
        });
        let Some(thread) = named.or(fallback) else {
            tracing::warn!(%matter, "a queued browser download names no thread of its matter");
            continue;
        };
        let first =
            state(actor, thread).is_none_or(|state| state.sources.fallbacks.first_time(&file.path));
        if first {
            log_notice(actor, thread, file.name);
        }
    }
}

/// The notice of the file `name`, logged on the thread once its stream is loaded.
fn log_notice(actor: &mut Actor, thread: ThreadId, name: String) {
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| {
                let mut out = Out::new(actor, thread, None)?;
                out.push(actor, fallback_notice(name))?;
                out.send(actor, Then::Nothing)
            });
            if let Err(error) = logged {
                tracing::warn!(%thread, error = %Cause(&error), "a download notice was not logged");
            }
            actor.release(stream);
        }),
    );
}
