//! The job's completion port (the outer's): the job posts `JOB_OBJECT_MSG_*` messages to it, and
//! a reader thread turns the first `ACTIVE_PROCESS_LIMIT` (`spawn::ACTIVE_PROCESS_CAP`)
//! into one notice for the lawyer. The session goes on: the job only refuses the new process.
//!
//! The notice is said on the outer's stderr today, which the host shows in its stderr tail. Once
//! #789's outer-to-lawyer session-notice path lands, the `notify` the outer passes moves onto it;
//! nothing else here changes.

/// `JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT`: a process was refused a place in the job because it
/// already holds its limit of active processes.
pub(super) const ACTIVE_PROCESS_LIMIT: u32 = 3;

/// What the lawyer sees, once per session, when the job refused a process at its cap.
pub(super) const TOO_MANY_PROGRAMS: &str = "A task tried to start too many programs at once, so \
                                            Clauseo stopped it. Ask the assistant to try a \
                                            smaller step.";

/// Reads `messages` (the port's, until it fails) and calls `notify` with [`TOO_MANY_PROGRAMS`] at
/// the first [`ACTIVE_PROCESS_LIMIT`] only: a fork bomb hits the cap over and over. One outer
/// serves one session, so once here is once per session.
pub(super) fn watch(messages: impl IntoIterator<Item = u32>, mut notify: impl FnMut(&str)) {
    let mut told = false;
    for message in messages {
        if message == ACTIVE_PROCESS_LIMIT && !told {
            told = true;
            notify(TOO_MANY_PROGRAMS);
        }
    }
}

#[cfg(windows)]
pub(super) mod os;

#[cfg(test)]
mod tests;
