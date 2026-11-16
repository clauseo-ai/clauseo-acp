//! The stetdoc read fence's own time limit: its walk runs on a thread of its own, and a walk not
//! done by [`WALK_DEADLINE`] (a mount that hangs) refuses the command, where Claude Code's limit
//! for the hook would run it unchecked.

use std::path::Path;
use std::time::Duration;

use futures::future::{Either, select};

use super::reads::{Fenced, GuardFence, fenced_read};
use super::shape::mentions_stetdoc;
use crate::clock::{Clock, SystemClock};
use crate::network_paths::NetEnv;
use crate::network_paths::resolve::Disk;
use crate::pre_write_guard::words::Escape;

/// The longest the fence's walk may take before the command is refused ([`Fenced::Late`]):
/// well inside the time Claude Code gives the hook (`agent::loads::GUARD_TIMEOUT_SECONDS`), past
/// which it runs the command unchecked. A walk on a mount that hangs never ends on its own.
pub(in crate::pre_write_guard) const WALK_DEADLINE: Duration = Duration::from_secs(7);

/// [`fenced_read`] on this system's disk, walked on a thread of its own ([`within`]). A command
/// that never mentions stetdoc, or an empty fence, answers at once.
pub(in crate::pre_write_guard) fn fenced_read_in_time(
    command: &str,
    escape: Escape,
    (base, matter): (&Path, &Path),
    fence: &GuardFence,
) -> Option<Fenced> {
    if !fence.is_on() || !mentions_stetdoc(command) {
        return None;
    }
    let (command, base, matter, fence) = (
        command.to_owned(),
        base.to_owned(),
        matter.to_owned(),
        fence.clone(),
    );
    within(&SystemClock, move || {
        // The network rules read again on this thread (the guard's can't be shared): the mount
        // list, never a path.
        let net = NetEnv::current();
        fenced_read(
            &command,
            escape,
            (&base, &matter),
            &fence,
            &Disk::real(&net),
        )
    })
}

/// `read`'s verdict, from a thread of its own; [`Fenced::Late`] when it gave none within
/// [`WALK_DEADLINE`] on `clock`, or the thread could not start or failed. A thread still
/// walking ends with the guard's process.
fn within(
    clock: &dyn Clock,
    read: impl FnOnce() -> Option<Fenced> + Send + 'static,
) -> Option<Fenced> {
    let (send, answer) = async_channel::bounded(1);
    let started = std::thread::Builder::new()
        .name("clauseo-stetdoc-fence".to_owned())
        .spawn(move || {
            // Past the deadline nobody is left to take the verdict.
            drop(send.send_blocking(read()));
        });
    if started.is_err() {
        return Some(Fenced::Late);
    }
    let answer = std::pin::pin!(answer.recv());
    match futures::executor::block_on(select(answer, clock.after(WALK_DEADLINE))) {
        Either::Left((Ok(verdict), _)) => verdict,
        Either::Left((Err(async_channel::RecvError), _)) | Either::Right(((), _)) => {
            Some(Fenced::Late)
        }
    }
}

#[cfg(test)]
#[path = "deadline_tests.rs"]
mod tests;
