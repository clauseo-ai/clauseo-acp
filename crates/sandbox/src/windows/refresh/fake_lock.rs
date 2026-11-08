//! The fake machine's run lock and "run again" flag (`wsl_phase/run_lock.rs`): another run's
//! hold as a count of tries it answers, a refused lock path, and other starts setting the flag
//! on the fake's clock.

use std::time::Duration;

use super::fake::Fake;
use super::{Failed, Locked, WslRunLock};

/// See the module.
#[derive(Debug, Default)]
pub(super) struct FakeLock {
    /// How many `run_lock` tries find it held by another run before it is free.
    pub(super) busy: u32,
    /// Every `run_lock` is refused with this: a junction, or a file another user owns.
    pub(super) refused: Option<Failed>,
    /// Once a try has found it busy, every later try is refused with this.
    pub(super) refused_after_busy: Option<Failed>,
    /// This run holds it.
    pub(super) held: bool,
    /// The flag.
    pub(super) again: bool,
    /// When other starts set the flag, once the clock reaches each.
    pub(super) asks: Vec<Duration>,
    /// Setting the flag fails.
    pub(super) ask_fails: bool,
    /// When each `take_again` found the flag set.
    pub(super) taken: Vec<Duration>,
}

/// `machine`'s flag, with the asks made by now.
fn flag(machine: &mut Fake) -> &mut bool {
    let clock = machine.watch.clock;
    let lock = &mut machine.run_lock;
    if lock.asks.iter().any(|at| *at <= clock) {
        lock.again = true;
    }
    lock.asks.retain(|at| *at > clock);
    &mut lock.again
}

impl WslRunLock for Fake {
    fn run_lock(&mut self) -> Locked {
        self.calls.push("run_lock");
        let lock = &mut self.run_lock;
        if let Some(why) = &lock.refused {
            return Locked::Refused(why.clone());
        }
        if lock.busy > 0 {
            lock.busy -= 1;
            lock.refused = lock.refused_after_busy.take();
            return Locked::Busy;
        }
        lock.held = true;
        Locked::Held
    }

    fn run_unlock(&mut self) {
        self.calls.push("run_unlock");
        self.run_lock.held = false;
    }

    fn ask_again(&mut self) -> Result<(), Failed> {
        self.calls.push("ask_again");
        if self.run_lock.ask_fails {
            return Err("RegSetValueExW failed (5)".to_owned());
        }
        self.run_lock.again = true;
        Ok(())
    }

    fn take_again(&mut self) -> Result<bool, Failed> {
        self.calls.push("take_again");
        let again = std::mem::take(flag(self));
        if again {
            let clock = self.watch.clock;
            self.run_lock.taken.push(clock);
        }
        Ok(again)
    }

    fn asked_again(&mut self) -> Result<bool, Failed> {
        self.calls.push("asked_again");
        Ok(*flag(self))
    }
}
