//! The outer's clearing of a previous session's runners before its logon (M1), apart from its
//! calls so the tests hold it on every OS.
//!
//! No runner opens another: the runners' process DACL grants the pool user nothing
//! (`token::runner_process_sddl`). A previous session's end sweeper may still be running, for up
//! to [`SWEEPER_LIMIT`], when the next launch for the same pool user begins. Before it logs on,
//! the outer, as the desktop user, which the runners' process DACL grants wait, query and end
//! (`token::DESKTOP_USER_RIGHTS`), finds every live runner of the pool user: a process with the
//! runner's image that it can open with those rights and whose user, read through the handle it
//! holds (`ProcessTelemetryIdInformation`), is the pool user. It waits for each up to
//! [`SWEEPER_LIMIT`], then ends it. What an ended sweeper didn't get to is the new inner's first
//! act's (`sweep.rs`, `Point::Start`).

use std::time::Duration;

use super::RunExit;

/// The longest an end sweeper runs (`sweeper.rs`); the outer waits that long for a previous one.
pub(crate) const SWEEPER_LIMIT: Duration = Duration::from_secs(60);

/// The outer's calls; Windows' are `sweep/os.rs`'s [`OsPrevious`](super::sweep::os::OsPrevious).
pub(crate) trait Previous {
    /// A live runner of the pool user, held open (its pid pinned).
    type Runner;
    /// Every live runner of the pool user.
    fn find(&mut self) -> Result<Vec<Self::Runner>, String>;
    /// Waits up to `limit` for `runner` to exit; whether it did.
    fn wait(&mut self, runner: &Self::Runner, limit: Duration) -> bool;
    /// Ends `runner` and waits for it to be gone.
    fn end(&mut self, runner: &Self::Runner) -> Result<(), String>;
}

/// Waits for every live runner of the pool user up to [`SWEEPER_LIMIT`], then ends it. A list
/// that can't be read fails the launch (`RunExit::Sweep`), and so does a runner that can't be
/// ended (`RunExit::Escapee`): none of the session runs beside a previous one.
pub(crate) fn clear<P: Previous>(calls: &mut P) -> Result<(), (RunExit, String)> {
    let runners = calls
        .find()
        .map_err(|why| (RunExit::Sweep, format!("a previous runner: {why}")))?;
    for runner in &runners {
        if calls.wait(runner, SWEEPER_LIMIT) {
            continue;
        }
        calls
            .end(runner)
            .map_err(|why| (RunExit::Escapee, format!("a previous runner: {why}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each call; the runners in `exits` exit within the wait.
    #[derive(Default)]
    struct Record {
        runners: Vec<u32>,
        exits: Vec<u32>,
        fail_find: bool,
        fail_end: bool,
        done: Vec<String>,
    }

    impl Previous for Record {
        type Runner = u32;
        fn find(&mut self) -> Result<Vec<u32>, String> {
            self.done.push("find".to_owned());
            if self.fail_find {
                return Err("snapshot".to_owned());
            }
            Ok(self.runners.clone())
        }
        fn wait(&mut self, runner: &u32, limit: Duration) -> bool {
            self.done
                .push(format!("wait {runner} {}s", limit.as_secs()));
            self.exits.contains(runner)
        }
        fn end(&mut self, runner: &u32) -> Result<(), String> {
            self.done.push(format!("end {runner}"));
            if self.fail_end {
                return Err("TerminateProcess failed (5)".to_owned());
            }
            Ok(())
        }
    }

    #[test]
    fn a_live_runner_is_waited_on_then_ended_at_the_limit() {
        let mut record = Record {
            runners: vec![7, 8],
            exits: vec![8],
            ..Record::default()
        };
        assert_eq!(clear(&mut record), Ok(()));
        // 7 outlives the wait and is ended; 8 exits by itself and is left.
        assert_eq!(record.done, ["find", "wait 7 60s", "end 7", "wait 8 60s"]);
    }

    #[test]
    fn no_runner_means_no_wait() {
        let mut record = Record::default();
        assert_eq!(clear(&mut record), Ok(()));
        assert_eq!(record.done, ["find"]);
    }

    #[test]
    fn a_list_or_an_end_that_fails_refuses_the_launch() {
        let mut record = Record {
            fail_find: true,
            ..Record::default()
        };
        assert_eq!(
            clear(&mut record),
            Err((RunExit::Sweep, "a previous runner: snapshot".to_owned()))
        );
        let mut record = Record {
            runners: vec![7],
            fail_end: true,
            ..Record::default()
        };
        assert_eq!(
            clear(&mut record),
            Err((
                RunExit::Escapee,
                "a previous runner: TerminateProcess failed (5)".to_owned()
            ))
        );
    }
}
