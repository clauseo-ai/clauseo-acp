//! The launch's check of the stop and sweep tasks before the pool user's logon
//! (`unclean::logon_check`'s first step): read once more when the Task Scheduler didn't answer,
//! healed without a prompt through the refresh task when missing or changed, and refused only
//! when still so.

use std::time::Duration;

use super::{Markers, Refused};
use crate::windows::refresh::TASK_UNCHECKED;

/// What a launch refused before the pool user's logon says when the tasks still can't be
/// checked after [`TASK_RETRY`].
pub const TASKS_UNCHECKED_AT_LAUNCH: &str = "Clauseo couldn't check its clean-up task, so it \
     didn't start the AI tool. Try again. If this keeps happening, restart your computer.";
/// How long the launch's check waits before it reads the tasks once more, when the Task
/// Scheduler didn't answer: long enough for a brief stall to pass, short against the host's 180 s.
pub const TASK_RETRY: Duration = Duration::from_millis(300);

/// The launch's check of the tasks before the pool user's logon: a task that is missing or
/// changed refuses it now (`RunExit::AwaitingSweep`, the readiness check's words), since its
/// session's end couldn't ask for the sweep (or stop) that the next launch would then wait on.
/// First, though, the refresh task is asked to put it back, without a prompt ([`healed`]),
/// and only a task still missing or changed after that refuses. One that can't be read (the
/// Task Scheduler not answering) is read once more after [`TASK_RETRY`]; still unreadable, the
/// launch is refused the same way, in [`TASKS_UNCHECKED_AT_LAUNCH`]'s words, since nothing says
/// its end could sweep.
pub(super) fn tasks_ready(fs: &mut impl Markers) -> Result<(), Refused> {
    let mut repair = fs.task_repair();
    if repair == Some(TASK_UNCHECKED) {
        fs.pause(TASK_RETRY);
        repair = fs.task_repair();
    }
    if let Some(words) = repair
        && words != TASK_UNCHECKED
    {
        repair = healed(fs, words, || {});
    }
    match repair {
        None => Ok(()),
        Some(words) if words == TASK_UNCHECKED => {
            Err(Refused::sweep(TASKS_UNCHECKED_AT_LAUNCH.to_owned()))
        }
        Some(words) => Err(Refused::sweep(words.to_owned())),
    }
}

/// How long a launch waits, at most, for the refresh task to put a missing or changed stop or
/// sweep task back ([`healed`]), the second run after a swap included.
pub const HEAL_WAIT: Duration = Duration::from_secs(20);
/// How often the launch reads the tasks again while it waits.
pub const HEAL_POLL: Duration = Duration::from_millis(500);
/// Every how many reads the launch asks for the refresh task's run again: one asked for while
/// a run goes on (the desktop's own at start, after an update) is dropped.
const HEAL_ASK_AGAIN: u32 = 6;

/// The no-prompt heal, for a stop or sweep task that is missing or changed (`words`): asks
/// for a run of `\Clauseo\SandboxRefresh` (as the lawyer, through its `FRFX` entry), whose
/// `--refresh` puts the tasks back as SYSTEM (`refresh::task_keep`), and reads the tasks again
/// every [`HEAL_POLL`] for up to [`HEAL_WAIT`] in all. The first run after an update is the old
/// copy's, which only swaps the new copy in: once the record's `AdminCopySha256` changes from
/// what it was before the first ask ([`Markers::admin_copy_stamp`]), the run is asked for once
/// more at once, which is the new copy's. `None` once the tasks are the set-up's; else what they
/// still need, `words` when the refresh task couldn't be started or the tasks can't be read.
/// `started` is called once the run is asked for.
fn healed(
    fs: &mut impl Markers,
    words: &'static str,
    started: impl FnOnce(),
) -> Option<&'static str> {
    let before = fs.admin_copy_stamp();
    if !fs.heal_tasks() {
        return Some(words);
    }
    started();
    let polls = u32::try_from(HEAL_WAIT.as_millis() / HEAL_POLL.as_millis()).unwrap_or(1);
    let mut still = words;
    let mut asked_after_swap = false;
    for poll in 1..=polls {
        fs.pause(HEAL_POLL);
        match fs.task_repair() {
            None => return None,
            Some(now) if now == TASK_UNCHECKED => return Some(still),
            Some(now) => still = now,
        }
        if !asked_after_swap && fs.admin_copy_stamp() != before {
            asked_after_swap = true;
            fs.heal_tasks();
        } else if poll % HEAL_ASK_AGAIN == 0 {
            fs.heal_tasks();
        }
    }
    Some(still)
}

/// What the host shows while it waits for [`heal_first`]'s heal.
pub const HEALING: &str = "Clauseo is finishing an update before it starts the AI tool.";

/// The heal of [`healed`], for the host before it starts the outer runner, so it can say why
/// the launch waits: reads the stop and sweep tasks and, when one is missing or changed, asks the
/// refresh task to put it back, calling `on_waiting` once the run is asked for (the host shows
/// [`HEALING`] until this returns), and waits up to [`HEAL_WAIT`]. Nothing is called when the
/// tasks are fine, can't be read, or the refresh task can't be started. What they still need,
/// `None` when nothing; the runner's own check before its logon stays the final gate.
pub fn heal_first(fs: &mut impl Markers, on_waiting: impl FnOnce()) -> Option<&'static str> {
    match fs.task_repair() {
        Some(words) if words != TASK_UNCHECKED => healed(fs, words, on_waiting),
        other => other,
    }
}
