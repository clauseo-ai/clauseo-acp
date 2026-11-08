//! The order the outer runner starts the inner runner in (M1), apart from its calls so the tests
//! hold it on every OS: the logon, the check of the inner's token user, the job, the shut of its
//! process and thread, and only then the resume. A step that fails after the logon kills the
//! suspended inner runner, which never runs.
//!
//! No step gives the inner a handle to its job: the job kills on close, which happens only when
//! its last handle closes, so the outer's must be the only one, and the inner and the agent end
//! with the outer however it ends.
//!
//! The outer sweeps nothing: it can't see the pool user's processes (`sweep.rs`). The inner's
//! first act once resumed is that sweep (`inner_flow.rs`), with nothing of the launch's run
//! before it.
//!
//! Once the session's end has begun (the git watch, armed before the logon, can end it at any
//! time), no inner is made, and none is resumed: the end is checked before the logon, again once
//! the inner is in the job, and again just before the resume. An end that begins after the inner
//! is in the job ends it with the job; one that began before was seen by the check after the
//! assign, since the end is known as begun before its path ends the job (`end.rs`). Either way
//! no agent runs outside the end's kill.

/// The outer's steps; Windows' are `outer/steps.rs`'s.
pub(crate) trait Steps {
    /// The suspended inner runner.
    type Inner;
    /// A failure, which ends the launch.
    type Error;
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
    /// Ends it after `error`.
    fn kill(&mut self, inner: &Self::Inner, error: &Self::Error);
    /// Fails once the session's end has begun (`SessionEnd::began`).
    fn not_ended(&mut self) -> Result<(), Self::Error>;
}

/// Runs the steps in order, the end checked before the logon, after the assign and before the
/// resume; the running inner runner, or the first failure.
pub(crate) fn start<S: Steps>(steps: &mut S) -> Result<S::Inner, S::Error> {
    steps.not_ended()?;
    let inner = steps.log_on()?;
    let started = steps
        .check_user(&inner)
        .and_then(|()| steps.assign(&inner))
        .and_then(|()| steps.not_ended())
        .and_then(|()| steps.shut(&inner))
        .and_then(|()| steps.not_ended())
        .and_then(|()| steps.resume(&inner));
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

    /// Records each step; fails the one named `fail`. The session's end begins as the step
    /// named `end_at` runs (`"before"`: before any), as a trigger on another thread would.
    #[derive(Default)]
    struct Record {
        done: Vec<String>,
        fail: Option<&'static str>,
        end_at: Option<&'static str>,
        ended: bool,
    }

    impl Record {
        fn step(&mut self, name: &str) -> Result<(), String> {
            self.done.push(name.to_owned());
            if self.end_at == Some(name) {
                self.ended = true;
            }
            if self.fail == Some(name) {
                return Err(name.to_owned());
            }
            Ok(())
        }
    }

    impl Steps for Record {
        type Inner = u32;
        type Error = String;
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
        fn kill(&mut self, inner: &u32, error: &String) {
            self.done.push(format!("kill {inner} after {error}"));
        }
        fn not_ended(&mut self) -> Result<(), String> {
            if self.end_at == Some("before") {
                self.ended = true;
            }
            self.done.push("not ended".to_owned());
            if self.ended {
                return Err("ended".to_owned());
            }
            Ok(())
        }
    }

    const ORDER: [&str; 8] = [
        "not ended",
        "log on",
        "check user",
        "assign",
        "not ended",
        "shut",
        "not ended",
        "resume",
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
        assert_eq!(record.done, ["not ended", "log on"]);
    }

    #[test]
    fn a_failure_after_logon_kills_the_inner_and_never_resumes_it() {
        for (at, step) in ORDER.iter().enumerate().skip(2) {
            if *step == "not ended" {
                continue;
            }
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

    /// The watch ended the session before the inner's start: no logon, nothing runs.
    #[test]
    fn an_end_before_the_start_makes_no_inner() {
        let mut record = Record {
            end_at: Some("before"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("ended".to_owned()));
        assert_eq!(record.done, ["not ended"]);
    }

    /// An end that begins at any step before the resume, the one between the check after the
    /// assign and the resume among them, kills the suspended inner, which never resumes. One
    /// that begins at the resume finds the inner in the job, whose end ends it.
    #[test]
    fn an_end_racing_the_start_is_caught_before_the_resume() {
        for (at, step) in ORDER.iter().enumerate().skip(1) {
            if *step == "not ended" || *step == "resume" {
                continue;
            }
            let mut record = Record {
                end_at: Some(step),
                ..Record::default()
            };
            assert_eq!(start(&mut record), Err("ended".to_owned()), "{step}");
            // The first check after the step that saw the end begin fails.
            let caught = ORDER
                .iter()
                .enumerate()
                .skip(at + 1)
                .find(|(_, s)| **s == "not ended")
                .map(|(i, _)| i)
                .unwrap();
            let mut want: Vec<String> = ORDER
                .iter()
                .take(caught + 1)
                .map(|s| (*s).to_owned())
                .collect();
            want.push("kill 7 after ended".to_owned());
            assert_eq!(record.done, want, "{step}");
            assert!(!record.done.iter().any(|s| s == "resume"), "{step}");
            // The inner was in the job before any check could pass: the job's end covers it.
            let assigned = record.done.iter().position(|s| s == "assign").unwrap();
            assert!(assigned < caught, "{step}");
        }
        let mut record = Record {
            end_at: Some("resume"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Ok(7));
        assert_eq!(record.done, ORDER);
    }
}
