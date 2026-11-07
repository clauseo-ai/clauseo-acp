//! The order the outer runner starts the inner runner in (M1), apart from its calls so the tests
//! hold it on every OS: sweep (a), the logon, the check of the inner's token user, the job, the
//! shut of its process and thread, sweep (b), and only then the resume. A step that fails after
//! the logon kills the suspended inner runner, which never runs; one before it ends the launch.

use super::sweep::Point;

/// The outer's steps; Windows' are `outer.rs`'s.
pub(crate) trait Steps {
    /// The suspended inner runner.
    type Inner;
    /// A failure, which ends the launch.
    type Error;
    /// Sweeps the pool user's processes at `point`, keeping `inner` when there is one.
    fn sweep(&mut self, point: Point, inner: Option<&Self::Inner>) -> Result<(), Self::Error>;
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
}

/// Runs the steps in order; the running inner runner, or the first failure.
pub(crate) fn start<S: Steps>(steps: &mut S) -> Result<S::Inner, S::Error> {
    steps.sweep(Point::BeforeLogon, None)?;
    let inner = steps.log_on()?;
    let started = steps
        .check_user(&inner)
        .and_then(|()| steps.assign(&inner))
        .and_then(|()| steps.shut(&inner))
        .and_then(|()| steps.sweep(Point::BeforeResume, Some(&inner)))
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
        fn sweep(&mut self, point: Point, inner: Option<&u32>) -> Result<(), String> {
            let name = match (point, inner) {
                (Point::BeforeLogon, None) => "sweep a",
                (Point::BeforeResume, Some(7)) => "sweep b",
                _ => "sweep wrong",
            };
            self.step(name)
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
        fn kill(&mut self, inner: &u32, error: &String) {
            self.done.push(format!("kill {inner} after {error}"));
        }
    }

    const ORDER: [&str; 7] = [
        "sweep a",
        "log on",
        "check user",
        "assign",
        "shut",
        "sweep b",
        "resume",
    ];

    #[test]
    fn the_inner_is_swept_around_before_it_runs() {
        let mut record = Record::default();
        assert_eq!(start(&mut record), Ok(7));
        assert_eq!(record.done, ORDER);
    }

    #[test]
    fn a_failed_sweep_before_logon_starts_no_logon() {
        let mut record = Record {
            fail: Some("sweep a"),
            ..Record::default()
        };
        assert_eq!(start(&mut record), Err("sweep a".to_owned()));
        assert_eq!(record.done, ["sweep a"]);
    }

    #[test]
    fn a_failure_after_logon_kills_the_inner_and_never_resumes_it() {
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
