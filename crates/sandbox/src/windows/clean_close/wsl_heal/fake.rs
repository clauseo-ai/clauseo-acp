//! A fake WSL deny task with a clock that moves only when the launch check sleeps, and checks
//! scripted against that clock.

use std::cell::Cell;
use std::time::Duration;

use super::DenyTask;
use crate::Blocked;
use crate::windows::refresh::last_run::LastRun;
use crate::windows::wsl::Found;

/// What happens at each time, from the time it names on: the last entry at or before `at`.
pub(crate) type Script<T> = Vec<(Duration, T)>;

/// The entry of `script` at `at`; the first when none is due yet.
fn due<T: Clone>(script: &Script<T>, at: Duration) -> T {
    let mut found = script.first().map(|(_, value)| value.clone());
    for (from, value) in script {
        if *from <= at {
            found = Some(value.clone());
        }
    }
    found.expect("a script has an entry")
}

/// The fake task.
pub(crate) struct FakeTask {
    /// What a request gives.
    pub(crate) request: Result<(), String>,
    /// The task's last run over time.
    pub(crate) last: Script<Result<LastRun, String>>,
    /// How many requests were made.
    pub(crate) requests: Cell<usize>,
    clock: Cell<Duration>,
}

impl FakeTask {
    /// A task that starts when asked and has never run.
    pub(crate) fn new() -> FakeTask {
        FakeTask {
            request: Ok(()),
            last: vec![(Duration::ZERO, Ok(LastRun::NeverRun))],
            requests: Cell::new(0),
            clock: Cell::new(Duration::ZERO),
        }
    }

    /// A task that isn't registered: every request fails, as on a machine without tier B.
    pub(crate) fn absent() -> FakeTask {
        let mut task = FakeTask::new();
        task.request = Err("the task isn't registered in \\Clauseo".to_owned());
        task.last = vec![(Duration::ZERO, Err("not registered".to_owned()))];
        task
    }

    /// The fake clock's time.
    pub(crate) fn elapsed_now(&self) -> Duration {
        self.clock.get()
    }

    /// A check that gives `script`'s entry at this task's time.
    pub(crate) fn checks(
        &self,
        script: Script<Result<Found, Blocked>>,
    ) -> impl FnMut() -> Result<Found, Blocked> + '_ {
        move || due(&script, self.clock.get())
    }
}

impl DenyTask for FakeTask {
    fn request(&self) -> Result<(), String> {
        self.requests.set(self.requests.get() + 1);
        self.request.clone()
    }

    fn last_run(&self) -> Result<LastRun, String> {
        due(&self.last, self.clock.get())
    }

    fn sleep(&self, time: Duration) {
        self.clock.set(self.clock.get() + time);
    }

    fn elapsed(&self) -> Duration {
        self.clock.get()
    }
}
