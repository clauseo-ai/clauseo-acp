//! The pre-write guard's launch self-test (`crate::guard_selftest` runs it): before each Claude
//! launch, the exact `PreToolUse` entry the session will get (`agent::loads::guard_entry`, with
//! `--ask-all` when the launch's folder answer puts it in forced ask) is run as Claude Code runs
//! it ([`Spawn::ClaudeExec`](crate::guard_selftest::Spawn::ClaudeExec)) on a made-up `Write`
//! that the guard refuses whatever the matter.
//!
//! The input ([`input`]): a `Write` of `clauseo-guard-self-test.txt` in the session's folder,
//! with `permission_mode` `bypassPermissions`. The guard refuses every tool it checks in that
//! mode (`Refusal::UnaskedMode`) before it looks at any file, so the refusal holds for every
//! matter, every file on disk or none, and forced ask or not; and the guard still runs its
//! whole path to that answer (stdin read, JSON parsed, targets found, mode checked). The file is
//! never written: the guard only reads its input.
//!
//! When the program is missing, cannot start, does not refuse with a reason, or is late twice
//! ([`crate::guard_selftest::LIMIT`]), the launch is refused with
//! `agent.guard_selftest_failed`. A hook that breaks later in the session is the guard's own
//! watchdog and panic refusal's to answer (`watchdog.rs`). The file-write guard's fence plugs in
//! beside this one (`file_fence::selftest` on its branch), each with its own refusing input.

use std::path::Path;
use std::sync::Arc;

use serde_json::Value;

use crate::clock::Clock;
use crate::error::Error;
use crate::guard_selftest::Failed;

/// The name of the file the made-up `Write` names, in the session's folder.
pub(crate) const FILE: &str = "clauseo-guard-self-test.txt";

/// The made-up hook input for the session folder `matter`: a `Write` the guard refuses in every
/// matter (the module doc).
pub(crate) fn input(matter: &Path) -> Vec<u8> {
    serde_json::json!({
        "session_id": "clauseo-self-test",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_use_id": "clauseo-self-test",
        "tool_input": {"file_path": matter.join(FILE), "content": ""},
        "cwd": matter,
        "permission_mode": "bypassPermissions",
    })
    .to_string()
    .into_bytes()
}

/// The entry a Claude session in the session folder `matter` gets from the guard program
/// `program`, in forced ask when `forced_ask`; `None` when it gets none.
pub(crate) fn entry(program: Option<&Path>, matter: &Path, forced_ask: bool) -> Option<Value> {
    program.and_then(|program| {
        crate::agent::loads::guard_entry(crate::agent::loads::GuardHook {
            program,
            matter,
            ask_all: forced_ask,
        })
    })
}

/// The self-test of a Claude launch whose guard program is `program`, its matter stored at
/// `stored`, the launch's folder answer `answer` (forced ask's entry when it puts the session
/// there, `forced_ask::applies`): the launch's refusal when it fails. Blocks the calling thread
/// (the launch prep's) for at most two of [`crate::guard_selftest::LIMIT`]. With `stop`, given up
/// as soon as it ends (the lawyer's Stop of the launch, `actor/turn/launch_checks.rs`): the
/// hook's process tree is ended and the answer is [`Error::Closed`], no refusal; without, it
/// runs to its answer.
pub(crate) fn for_launch(
    program: Option<&Path>,
    stored: &Path,
    answer: Option<crate::agent::folder_support::Answer>,
    clock: &Arc<dyn Clock>,
    stop: Option<impl Future<Output = ()>>,
) -> Result<(), Error> {
    let forced_ask = answer.is_some_and(crate::agent::forced_ask::applies);
    let matter = crate::agent::session_folder(stored);
    let entry = entry(program, &matter, forced_ask);
    let input = input(&matter);
    // Claude Code's spawn (`run` is `run_as(Spawn::ClaudeExec, …)`).
    let ran = match stop {
        Some(stop) => {
            crate::guard_selftest::run_until(entry.as_ref(), &input, &matter, clock, stop)
        }
        None => crate::guard_selftest::run(entry.as_ref(), &input, &matter, clock),
    };
    match ran {
        Err(Failed::Stopped) => {
            tracing::info!("a guard's self-test was given up: its launch was stopped");
            Err(Error::Closed)
        }
        ran => ran.map_err(|failed: Failed| crate::guard_selftest::refusal(failed, "Claude")),
    }
}

/// For the desktop's test against the built app (`crates/desktop/tests/guard_selftest.rs`):
/// the self-test of the guard program `program` for the matter folder `stored`, in forced ask
/// when `forced_ask`, and the time it took by the system clock. `Err` names the failure's kind.
#[cfg(feature = "test-support")]
pub fn timed_for_test(
    program: &Path,
    stored: &Path,
    forced_ask: bool,
) -> (Result<(), String>, std::time::Duration) {
    let clock: Arc<dyn Clock> = Arc::new(crate::clock::SystemClock);
    let started = clock.now().mono;
    let matter = crate::agent::session_folder(stored);
    let entry = entry(Some(program), &matter, forced_ask);
    let input = input(&matter);
    let ran = crate::guard_selftest::run_as(
        crate::guard_selftest::Spawn::ClaudeExec,
        entry.as_ref(),
        &input,
        &matter,
        &clock,
    );
    let took = clock.now().mono.saturating_sub(started);
    (ran.map_err(|failed| format!("{failed:?}")), took)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::{entry, input};
    use crate::guard_selftest::command_of;

    /// The input the guard refuses in every matter: the unasked-mode belt, with no file on disk,
    /// in both entries' argument shapes (forced ask or not).
    #[test]
    fn the_guard_refuses_the_made_up_input_with_a_reason_in_every_entry_shape() {
        // The file need not exist: the refusal comes before any file is looked at.
        let temp = std::env::temp_dir();
        let matter = temp.as_path();
        for forced_ask in [false, true] {
            let program = Path::new("/opt/clauseo/clauseo-desktop");
            let entry = entry(Some(program), matter, forced_ask).unwrap();
            let (command, args) = command_of(&entry).unwrap();
            assert_eq!(command, program);
            // What the guard's real entry gets after its flag.
            let (flag, rest) = args.split_first().unwrap();
            assert_eq!(flag, &OsString::from(super::super::FLAG));
            let mut errors = Vec::new();
            let code =
                super::super::run_timed(rest, &input(matter)[..], (Vec::new(), &mut errors), false);
            let errors = String::from_utf8(errors).unwrap();
            assert_eq!(code, super::super::DENY, "{errors}");
            assert_ne!(errors.trim(), "");
        }
    }

    #[test]
    fn no_program_gives_no_entry() {
        assert_eq!(entry(None, Path::new("/m"), false), None);
    }
}
