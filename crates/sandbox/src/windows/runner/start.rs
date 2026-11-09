//! The order the outer runner starts the inner runner in (M1), apart from its calls so the tests
//! hold it on every OS: a previous session's runners waited for and ended (`previous.rs`), the
//! logon, the check of the inner's token user, the job, the shut of its
//! process and thread, the resume, and the full check of its token. A step that fails after the
//! logon kills the inner runner: before the resume it never runs.
//!
//! The check is in two stages. Before the resume, the user alone (`check_user`): on the agent's
//! own station the inner's token doesn't grant the outer `TOKEN_QUERY`, and the user is read
//! from the process instead (`inner_user.rs`). After the resume the inner's one act is to grant
//! the desktop user `TOKEN_QUERY` on its token, say it is ready on `ctl`, and wait; the outer
//! then opens the token and checks it (`confirm`: the user again, a logon SID of the run's own,
//! not the desktop user's, Medium integrity, no full elevation; not its groups or restricting
//! SIDs) before it sends the go. Until the go the inner has done
//! nothing but the grant (`inner_flow.rs`).
//!
//! No step gives the inner a handle to its job: the job kills on close, which happens only when
//! its last handle closes, so the outer's must be the only one, and the inner and the agent end
//! with the outer however it ends.
//!
//! The outer sweeps nothing: it can't see the pool user's processes (`sweep.rs`). The inner's
//! first act once resumed is that sweep (`inner_flow.rs`), with nothing of the launch's run
//! before it.

/// The outer's steps; Windows' are `outer/steps.rs`'s.
pub(crate) trait Steps {
    /// The suspended inner runner.
    type Inner;
    /// A failure, which ends the launch.
    type Error;
    /// Waits for, then ends, a previous session's live runners of the pool user.
    fn clear(&mut self) -> Result<(), Self::Error>;
    /// Starts the inner runner as the pool user, suspended.
    fn log_on(&mut self) -> Result<Self::Inner, Self::Error>;
    /// Checks its token's user is the pool user.
    fn check_user(&mut self, inner: &Self::Inner) -> Result<(), Self::Error>;
    /// Puts it in the kill-on-close job.
    fn assign(&mut self, inner: &Self::Inner) -> Result<(), Self::Error>;
    /// Shuts its process and first thread to the pool user.
    fn shut(&mut self, inner: &Self::Inner) -> Result<(), Self::Error>;
    /// Lets it run.
    fn resume(&mut self, inner: &Self::Inner) -> Result<(), Self::Error>;
    /// Waits for its ready, checks its token (`inner_user::check_full`), and sends it the go.
    fn confirm(&mut self, inner: &Self::Inner) -> Result<(), Self::Error>;
    /// Ends it after `error`.
    fn kill(&mut self, inner: &Self::Inner, error: &Self::Error);
}

/// Runs the steps in order; the running inner runner, or the first failure.
pub(crate) fn start<S: Steps>(steps: &mut S) -> Result<S::Inner, S::Error> {
    steps.clear()?;
    let inner = steps.log_on()?;
    let started = steps
        .check_user(&inner)
        .and_then(|()| steps.assign(&inner))
        .and_then(|()| steps.shut(&inner))
        .and_then(|()| steps.resume(&inner))
        .and_then(|()| steps.confirm(&inner));
    match started {
        Ok(()) => Ok(inner),
        Err(error) => {
            steps.kill(&inner, &error);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records each step; fails the one named `fail`.
    #[derive(Default)]
    struct Record {
        done: Vec<String>,
        fail: Option<&'static str>,
    }

    impl Record {
        fn step(&mut self, name: &str) -> Result<(), String> {
            self.done.push(name.to_owned());
            if self.fail == Some(name) {
                return Err(name.to_owned());
            }
            Ok(())
        }
    }

    impl Steps for Record {
        type Inner = u32;
        type Error = String;
        fn clear(&mut self) -> Result<(), String> {
            self.step("clear")
        }
        fn log_on(&mut self) -> Result<u32, String> {
            self.step("log on").map(|()| 7)
        }
        fn check_user(&mut self, _: &u32) -> Result<(), String> {
            self.step("check user")
        }
        fn assign(&mut self, _: &u32) -> Result<(), String> {
            self.step("assign")
        }
        fn shut(&mut self, _: &u32) -> Result<(), String> {
            self.step("shut")
        }
        fn resume(&mut self, _: &u32) -> Result<(), String> {
            self.step("resume")
        }
        fn confirm(&mut self, _: &u32) -> Result<(), String> {
            self.step("confirm")
        }
        fn kill(&mut self, inner: &u32, error: &String) {
            self.done.push(format!("kill {inner} after {error}"));
        }
    }

    const ORDER: [&str; 7] = [
        "clear",
        "log on",
        "check user",
        "assign",
        "shut",
        "resume",
        "confirm",
    ];

    /// The steps are the whole start: none hands the inner a handle (to its job or anything).
    #[test]
    fn the_inner_is_put_in_its_job_and_shut_before_it_runs_and_given_no_handle() {
        let mut record = Record::default();
        assert_eq!(start(&mut record), Ok(7));
        assert_eq!(record.done, ORDER);
    }

    #[test]
    fn a_failed_logon_kills_nothing() {
        let mut record = Record {
            fail: Some("log on"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("log on".to_owned()));
        assert_eq!(record.done, ["clear", "log on"]);
    }

    /// A previous runner that couldn't be waited for or ended refuses the launch before any
    /// logon: nothing of this session runs beside it.
    #[test]
    fn a_failed_clear_logs_nothing_on() {
        let mut record = Record {
            fail: Some("clear"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("clear".to_owned()));
        assert_eq!(record.done, ["clear"]);
    }

    #[test]
    /// Each failure kills the inner: before the resume it never ran, and a failed full check
    /// (`confirm`) kills it before its go, with nothing done but its token's grant.
    fn a_failure_after_logon_kills_the_inner() {
        for (at, step) in ORDER.iter().enumerate().skip(2) {
            let mut record = Record {
                fail: Some(step),
                ..Record::default()
            };
            assert_eq!(start(&mut record), Err((*step).to_owned()));
            let mut want: Vec<String> =
                ORDER.iter().take(at + 1).map(|s| (*s).to_owned()).collect();
            want.push(format!("kill 7 after {step}"));
            assert_eq!(record.done, want, "{step}");
        }
    }
}
