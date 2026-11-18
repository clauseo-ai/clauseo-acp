//! The thread or matter a command names, or the refusal that it is unknown.

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};

use super::super::{Actor, MatterEntry, ThreadEntry};
use crate::error::Error;

/// The thread `thread`, or `command.unknown_thread`.
pub(crate) fn check_thread(actor: &Actor, thread: ThreadId) -> Result<&ThreadEntry, Error> {
    actor.threads.get(&thread).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandUnknownThread,
            format!("no thread {thread}"),
        )
    })
}

/// The thread `thread` of a matter on the list, for a command that names it:
/// `command.unknown_thread` for an unknown thread or one of a forgotten matter.
pub(in crate::actor) fn listed_thread(
    actor: &Actor,
    thread: ThreadId,
) -> Result<&ThreadEntry, Error> {
    let entry = actor.check_thread(thread)?;
    if actor.matters.contains_key(&entry.row.matter) {
        Ok(entry)
    } else {
        Err(Error::refused(
            ErrorCode::CommandUnknownThread,
            format!("thread {thread} is in a forgotten matter"),
        ))
    }
}

/// The matter `matter`, or `command.unknown_matter`.
pub(crate) fn check_matter(actor: &Actor, matter: MatterId) -> Result<&MatterEntry, Error> {
    actor.matters.get(&matter).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandUnknownMatter,
            format!("no matter {matter}"),
        )
    })
}
