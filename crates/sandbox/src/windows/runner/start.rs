//! The order the outer runner starts the inner runner in (M1), apart from its calls so the tests
//! hold it on every OS: the host's denial pipe connected (its one instance taken, so nothing of
//! the agent's can race for it; one named that can't be opened refuses the launch), the git watch
//! armed, a previous session's runners waited for and ended (`previous.rs`), the logon, the
//! check of the inner's token user, the job, the shut of its process and thread, the resume, and
//! the full check of its token. A step that fails after the logon kills the inner runner: before
//! the resume it never runs.
//!
//! The previous runners are ended after the pipe and the watch: neither runs anything as the pool
//! user, and the end check that follows still sees an end the watch began meanwhile.
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
//!
//! Once the session's end has begun (the git watch, armed before the logon, can end it at any
//! time), no inner is made, and none is resumed: the end is checked before the logon, again once
//! the inner is in the job, and again just before the resume. An end that begins after the inner
//! is in the job ends it with the job; one that began before was seen by the check after the
//! assign, since the end is known as begun before its path ends the job (`end.rs`). Either way
//! no agent runs outside the end's kill.

/// The outer's steps; Windows' are `outer/steps.rs`'s.
pub(crate) trait Steps {
    /// Connects the host's denial pipe, when the launch names one (`denial_pipe`).
    fn feed(&mut self) -> Result<(), Self::Error>;
    /// Arms the git watch, whose records go to that pipe (`outer/watch.rs`).
    fn watch(&mut self) -> Result<(), Self::Error>;
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
    /// Fails once the session's end has begun (`SessionEnd::began`).
    fn not_ended(&mut self) -> Result<(), Self::Error>;
}

/// Runs the steps in order: the feed and the watch first, then the end checked before the logon,
/// after the assign and before the resume; the running inner runner, or the first failure.
pub(crate) fn start<S: Steps>(steps: &mut S) -> Result<S::Inner, S::Error> {
    steps.feed()?;
    steps.watch()?;
    steps.clear()?;
    steps.not_ended()?;
    let inner = steps.log_on()?;
    let started = steps
        .check_user(&inner)
        .and_then(|()| steps.assign(&inner))
        .and_then(|()| steps.not_ended())
        .and_then(|()| steps.shut(&inner))
        .and_then(|()| steps.not_ended())
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
        fn feed(&mut self) -> Result<(), String> {
            self.step("feed")
        }
        fn watch(&mut self) -> Result<(), String> {
            self.step("watch")
        }
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

    const ORDER: [&str; 12] = [
        "feed",
        "watch",
        "clear",
        "not ended",
        "log on",
        "check user",
        "assign",
        "not ended",
        "shut",
        "not ended",
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
        assert_eq!(
            record.done,
            ["feed", "watch", "clear", "not ended", "log on"]
        );
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
        assert_eq!(record.done, ["feed", "watch", "clear"]);
    }

    /// The denial pipe is connected before anything else, and one that can't be opened (taken,
    /// gone) refuses the launch: no watch, no logon, no inner. So does a watch that can't arm.
    #[test]
    fn the_feed_is_connected_first_and_a_failed_one_starts_no_inner() {
        let mut record = Record {
            fail: Some("feed"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("feed".to_owned()));
        assert_eq!(record.done, ["feed"]);
        let mut record = Record {
            fail: Some("watch"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("watch".to_owned()));
        assert_eq!(record.done, ["feed", "watch"]);
        let mut record = Record::default();
        assert_eq!(start(&mut record), Ok(7));
        let at = |name: &str| record.done.iter().position(|s| s == name).unwrap();
        assert!(
            at("feed") < at("watch") && at("watch") < at("clear") && at("clear") < at("log on")
        );
    }

    /// Each failure kills the inner: before the resume it never ran, and a failed full check
    /// (`confirm`) kills it before its go, with nothing done but its token's grant.
    #[test]
    fn a_failure_after_logon_kills_the_inner_and_never_resumes_it() {
        for (at, step) in ORDER.iter().enumerate().skip(5) {
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
        assert_eq!(record.done, ["feed", "watch", "clear", "not ended"]);
        // The watch, once armed, ended it before the logon (as the previous runners were ended,
        // or before): the same.
        for end_at in ["watch", "clear"] {
            let mut record = Record {
                end_at: Some(end_at),
                ..Record::default()
            };
            assert_eq!(start(&mut record), Err("ended".to_owned()), "{end_at}");
            assert_eq!(
                record.done,
                ["feed", "watch", "clear", "not ended"],
                "{end_at}"
            );
        }
    }

    /// An end that begins at any step before the resume, the one between the check after the
    /// assign and the resume among them, kills the suspended inner, which never resumes. One
    /// that begins at the resume finds the inner in the job, whose end ends it.
    #[test]
    fn an_end_racing_the_start_is_caught_before_the_resume() {
        for (at, step) in ORDER.iter().enumerate().skip(4) {
            if matches!(*step, "not ended" | "resume" | "confirm") {
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
