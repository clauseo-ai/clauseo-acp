//! The end sweeper's order (`sweeper.rs`), behind [`SweeperActs`] so the tests can follow it:
//!
//! 1. **Shut**: its primary token to `SYSTEM` alone, then its default DACL and threads.
//! 2. **Sweep**: both passes over its user's processes (`sweep::end_sweep`), the token still shut,
//!    since a process of the pool user could still be alive to open it.
//! 3. **Widen**: only once the sweep left no process of its user (it went without a failure),
//!    the token gives its user `TOKEN_QUERY`, `TOKEN_DUPLICATE` and `TOKEN_IMPERSONATE`
//!    (`token::SWEEPER_COM_TOKEN_RIGHTS`), what COM needs: with the token shut, Task Scheduler
//!    fails with `E_OUTOFMEMORY` (14).
//! 4. **Remove** (`leftovers::remove_all`): the per-user COM classes first, the `Run` and
//!    `RunOnce` values and the Startup folder, then the scheduled tasks over COM. No BITS job is
//!    touched (`leftovers.rs` says why).
//!
//! A sweep that failed, or a widening that did, leaves the token shut: the classes, values and
//! Startup entries are removed without COM, and the tasks wait for the next session's end
//! sweeper.

use std::io;

use super::RunExit;
use super::inner_flow::at;
use super::leftovers::Kind;
use super::sweep::{Point, SweepError, end_sweeper_exit};

/// The end sweeper's acts.
pub(crate) trait SweeperActs {
    /// Shuts its token, then its default DACL and threads; what couldn't be done, and why.
    fn shut(&mut self) -> Result<(), (&'static str, io::Error)>;
    /// Both passes of the sweep of its user's processes.
    fn sweep(&mut self, log: &mut dyn FnMut(&str)) -> Result<(), SweepError>;
    /// Gives its user the token rights COM needs.
    fn widen_for_com(&mut self) -> io::Result<()>;
    /// Removes `kinds` (`leftovers::remove_all`); whether everything went.
    fn remove(&mut self, kinds: &[Kind], log: &mut dyn FnMut(&str)) -> bool;
}

/// The end sweeper's run after its start, in order; its exit code.
pub(crate) fn run<S: SweeperActs>(steps: &mut S, log: &mut dyn FnMut(&str)) -> u32 {
    if let Err((what, error)) = steps.shut() {
        log(&format!(
            "clauseo-sandbox-run: the end sweeper can't {what} ({})",
            code(&error)
        ));
        return RunExit::Sweep.code();
    }
    let kills = steps.sweep(log);
    let kinds: &[Kind] = match &kills {
        Err(error) => {
            log(&format!(
                "clauseo-sandbox-run: {}",
                at(Point::Sweeper, error)
            ));
            log(
                "clauseo-sandbox-run: cleanup (end sweeper): the scheduled tasks weren't listed: \
                 the sweep may have left a process of the sandbox user, so its token stays shut",
            );
            &Kind::LOCAL
        }
        Ok(()) => match steps.widen_for_com() {
            Ok(()) => &Kind::ALL,
            Err(error) => {
                log(&format!(
                    "clauseo-sandbox-run: cleanup (end sweeper): the scheduled tasks weren't \
                     listed: its token can't be opened to COM ({})",
                    code(&error)
                ));
                &Kind::LOCAL
            }
        },
    };
    let removed = steps.remove(kinds, log) && kinds == Kind::ALL.as_slice();
    end_sweeper_exit(&kills, removed)
}

/// The verify-only pass's run (`--verify`, `quarantine.rs`): shut, then the sweep's two passes,
/// nothing removed and the token never widened; its exit code: 0 when the sweep left no process of
/// its user, `RunExit::Escapee` when it left an escapee (a survivor among them), else
/// `RunExit::Sweep`.
pub(crate) fn verify<S: SweeperActs>(steps: &mut S, log: &mut dyn FnMut(&str)) -> u32 {
    if let Err((what, error)) = steps.shut() {
        log(&format!(
            "clauseo-sandbox-run: the verify pass can't {what} ({})",
            code(&error)
        ));
        return RunExit::Sweep.code();
    }
    let kills = steps.sweep(log);
    match &kills {
        Ok(()) => 0,
        Err(error) => {
            log(&format!(
                "clauseo-sandbox-run: verify pass: {}",
                at(Point::Sweeper, error)
            ));
            end_sweeper_exit(&kills, true)
        }
    }
}

/// An error's OS code, or its kind.
fn code(error: &io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string())
}

#[cfg(test)]
mod tests;
