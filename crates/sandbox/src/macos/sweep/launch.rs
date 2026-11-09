//! [`LaunchSweep`]: the process cap's view of one launch's mark.

use std::sync::Arc;
use std::thread;

use super::{
    END_PAUSE, ProcId, Scan, SessionMark, SweepError, all, api, micros, now_wall, scan, seatbelt,
    settle, start_of,
};
use crate::macos::process_cap::Proc;
use crate::macos::process_cap::libproc::Libproc;
use crate::macos::process_cap::watchdog::{Escaped, Escapes, System as _};

/// How many rounds the hand-over runs before it leaves the rest to the watchdog's next sweep.
const TAKE_OVER_ROUNDS: usize = 20;

/// The process cap's view of one launch: its mark, and nothing else. The watchdog of that
/// launch's agent holds it (`process_cap/watchdog.rs`), so its sweep and its hand-over can
/// only ever find that launch's processes: the type holds one mark, and the mark one launch.
#[derive(Debug)]
pub(crate) struct LaunchSweep {
    mark: Arc<SessionMark>,
}

impl LaunchSweep {
    pub(crate) fn new(mark: Arc<SessionMark>) -> LaunchSweep {
        LaunchSweep { mark }
    }

    /// The launch's processes among `all` but its agent, with no start filter, and the scan's
    /// doubt.
    fn found(&self, all: &[Proc]) -> Result<Scan, SweepError> {
        let mut scan = settle(
            scan(
                api()?,
                &[&self.mark],
                0,
                micros(now_wall()),
                seatbelt::uid(),
                all,
            )?,
            &[&self.mark],
        );
        let root = self.mark.root();
        scan.found.retain(|id| Some(*id) != root);
        Ok(scan)
    }
}

impl Escapes for LaunchSweep {
    /// The processes among `rest` of this launch, the agent and the sentinel left out: those
    /// that left the agent's tree and group are found by the mark. With no start filter, so a
    /// clock set back hides none. An unvouched scan answers what it found with its doubt.
    /// `Ok(None)` once session end ended the mark.
    fn escaped(&self, rest: &[Proc]) -> Result<Option<Escaped>, SweepError> {
        if self.mark.ended() {
            return Ok(None);
        }
        match self.found(rest) {
            // Session end ended the mark (and its sentinel) between the look and the scan.
            Ok(Scan { doubt: Some(_), .. }) | Err(_) if self.mark.ended() => Ok(None),
            Ok(scan) => Ok(Some(Escaped {
                found: rest
                    .iter()
                    .filter(|p| scan.found.contains(&ProcId::of(p)))
                    .copied()
                    .collect(),
                doubt: scan.doubt,
            })),
            Err(error) => Err(error),
        }
    }

    /// Restarts the launch's sentinel if it is dead ([`SessionMark::revive`]).
    fn revive(&self) -> bool {
        api().is_ok_and(|check| self.mark.revive(check))
    }

    /// What a trip's kill rounds left running (`left`), and every process of this launch the
    /// mark still finds (the agent and the sentinel left out; an unvouched scan's finds too),
    /// stopped and then killed, each by its start, for up to [`TAKE_OVER_ROUNDS`] rounds.
    /// Answers how many still run; the watchdog's next sweep counts them again.
    fn take_over(&self, left: &[Proc]) -> usize {
        let mut targets: Vec<ProcId> = left.iter().map(ProcId::of).collect();
        for _ in 0..TAKE_OVER_ROUNDS {
            if let Some(all) = all()
                && let Ok(scan) = self.found(&all)
            {
                for id in scan.found {
                    if !targets.contains(&id) {
                        targets.push(id);
                    }
                }
            }
            targets.retain(|id| start_of(id.pid) == Some(id.start));
            if targets.is_empty() {
                return 0;
            }
            for id in &targets {
                Libproc.signal(id.pid, id.start, libc::SIGSTOP);
            }
            for id in &targets {
                Libproc.signal(id.pid, id.start, libc::SIGKILL);
            }
            thread::park_timeout(END_PAUSE);
        }
        targets.retain(|id| start_of(id.pid) == Some(id.start));
        targets.len()
    }
}
