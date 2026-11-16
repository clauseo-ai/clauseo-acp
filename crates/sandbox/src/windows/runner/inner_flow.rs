//! The inner runner's order (M1), apart from its calls so the tests hold it on every OS. Once the
//! outer resumes it, and before it reads the launch or makes anything:
//!
//! 1. it sweeps its own user's processes ([`Point::Start`]), keeping only itself: a process of
//!    the pool user's that opened this runner's process, thread or token between the logon and
//!    the outer's shut, or one this logon set off (a task, a BITS job), is killed before any code
//!    of the launch's runs. Its failure fails the launch closed and starts no agent:
//!    [`RunExit::Escapee`] when a process couldn't be opened or ended (reported on `ctl` too,
//!    held until it opens), else [`RunExit::Sweep`];
//! 2. it removes what the pool user could have left to start at a logon or to run in a runner
//!    (`leftovers.rs`, `Kind::LOCAL`): its per-user COM classes (`HKCU\Software\Classes`), its
//!    `Run` and `RunOnce` values and its Startup entries. Nothing else removes them, and no code
//!    of the launch's runs before this (the agent starts in 4); the pool user's scheduled tasks
//!    and BITS jobs are the `SYSTEM` sweep task's (`end.rs`). Its failure is logged;
//! 3. it opens `ctl`, sends what 1 and 2 logged, and reads the launch;
//! 4. it starts the agent, then sweeps every [`TICK`] while the agent runs ([`Point::Tick`]) and
//!    once more when it exited ([`Point::End`]), both keeping its own job's processes, which it
//!    asks for by pid list with no handle to the job (`sweep.rs`); a failure there
//!    is logged, and a tick's is tried again at the next. An escapee there is reported on `ctl`
//!    ([`InnerReport::Escapee`]), which makes the outer exit [`RunExit::Escapee`]; the run goes
//!    on.
//!
//! No code of this runner's runs before 1 but the DLL search's limit, the error mode and the shut
//! of its own token, and the only token anyone could have opened before then is this runner's
//! own, as the pool user it already is.

use std::time::Duration;

use super::RunExit;
use super::frame::InnerReport;
use super::sweep::{Point, SweepError, TICK};

/// A failure before the agent ran: the exit code and what failed (a call and its code).
pub(crate) type Failure = (RunExit, String);

/// The inner's calls; Windows' are `inner.rs`'s.
pub(crate) trait Inner {
    /// The running agent.
    type Agent;
    /// Sweeps this user's processes at `point`, keeping this process and, past the first act,
    /// its job's; each kill goes to [`Inner::log`]. It is given nothing of the job: this runner
    /// holds no handle to it.
    fn sweep(&mut self, point: Point) -> Result<(), SweepError>;
    /// Removes the leftovers (`leftovers::remove_all`), logging their counts.
    fn clean(&mut self);
    /// A line for the outer: held until `ctl` is open, then sent on it.
    fn log(&mut self, line: &str);
    /// A report for the outer that may come before `ctl` is open (`Escapee`): held until then,
    /// in order with the lines, then sent on it.
    fn tell(&mut self, report: InnerReport);
    /// Opens `ctl` and sends the lines held so far.
    fn connect(&mut self) -> Result<(), RunExit>;
    /// Reads the launch and shuts this runner to the agent.
    fn read_launch(&mut self) -> Result<(), Failure>;
    /// Makes the agent's token and folders and starts it.
    fn start(&mut self) -> Result<Self::Agent, Failure>;
    /// The agent's pid.
    fn pid(&self, agent: &Self::Agent) -> u32;
    /// Waits up to `interval` for the agent; its exit code once it exited.
    fn wait(&mut self, agent: &Self::Agent, interval: Duration) -> Option<u32>;
    /// Ends the agent with `code`.
    fn kill(&mut self, agent: &Self::Agent, code: u32);
    /// Sends `report` on `ctl`; whether it went.
    fn report(&mut self, report: &InnerReport) -> bool;
}

/// The inner runner's run after its token is shut; its exit code: the agent's, or a
/// [`RunExit`] code.
pub(crate) fn run<I: Inner>(inner: &mut I) -> u32 {
    let swept = inner.sweep(Point::Start);
    match &swept {
        Ok(()) => inner.clean(),
        Err(error) => escaped(inner, Point::Start, error),
    }
    if let Err(exit) = inner.connect() {
        return swept
            .map_or_else(|error| start_exit(&error), |()| exit)
            .code();
    }
    if let Err(error) = swept {
        return failed(inner, (start_exit(&error), at(Point::Start, &error)));
    }
    if let Err(failure) = inner.read_launch() {
        return failed(inner, failure);
    }
    let agent = match inner.start() {
        Ok(agent) => agent,
        Err(failure) => return failed(inner, failure),
    };
    let pid = inner.pid(&agent);
    if !inner.report(&InnerReport::Started { pid }) {
        inner.kill(&agent, RunExit::Pipe.code());
    }
    let code = loop {
        if let Some(code) = inner.wait(&agent, TICK) {
            break code;
        }
        if let Err(error) = inner.sweep(Point::Tick) {
            inner.log(&format!("clauseo-sandbox-run: {}", at(Point::Tick, &error)));
            escaped(inner, Point::Tick, &error);
        }
    };
    if let Err(error) = inner.sweep(Point::End) {
        inner.log(&format!("clauseo-sandbox-run: {}", at(Point::End, &error)));
        escaped(inner, Point::End, &error);
    }
    // The outer may be gone already; the exit code says it anyway.
    inner.report(&InnerReport::Exited { code });
    code
}

/// Reports `failure` (the outer may be gone already) and gives its exit code.
fn failed<I: Inner>(inner: &mut I, (exit, what): Failure) -> u32 {
    inner.report(&InnerReport::Failed {
        exit: exit.code(),
        what,
    });
    exit.code()
}

/// The first act's exit code for its sweep's `error`: [`RunExit::Escapee`] when it names a
/// process that couldn't be opened or ended, else [`RunExit::Sweep`].
fn start_exit(error: &SweepError) -> RunExit {
    if error.escapees().is_some() {
        RunExit::Escapee
    } else {
        RunExit::Sweep
    }
}

/// Tells the outer the escapees a sweep's `error` at `point` names, if any.
fn escaped<I: Inner>(inner: &mut I, point: Point, error: &SweepError) {
    if let Some(escapees) = error.escapees() {
        inner.tell(InnerReport::Escapee {
            point: point.to_string(),
            count: u32::try_from(escapees.count).unwrap_or(u32::MAX),
            images: escapees.images,
        });
    }
}

/// A sweep's failure at `point`, as the log says it.
pub(crate) fn at(point: Point, error: &SweepError) -> String {
    format!("sweep ({point}): {error}")
}

#[cfg(test)]
mod tests;
