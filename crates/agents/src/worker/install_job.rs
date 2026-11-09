//! The worker's start of an install job: the running job stopped, `agent`'s on its own thread.

use std::sync::{Arc, Mutex};

use clauseo_process::Timer;
use clauseo_protocol::AgentId;

use super::{Ticket, Worker};
use crate::cancel::Cancel;
use crate::install::{self, Job};
use crate::log::{InstallLog, redact};
use crate::state::{Failure, FailureKind, InstallState};

/// Stops the running job (pausing another agent's), then starts `agent`'s on its own
/// thread; that thread waits for the job lock before it works.
pub(super) fn start<T: Timer + Send + Sync + 'static>(worker: &Worker<T>, agent: &AgentId) {
    let ticket = {
        let mut inner = worker.shared.lock();
        if inner.closed {
            return;
        }
        inner.stop_current(&worker.shared.events, Some(agent));
        inner.next_job += 1;
        let ticket = Ticket {
            job: inner.next_job,
            agent: agent.clone(),
            cancel: Arc::new(Cancel::new()),
        };
        inner.current = Some(ticket.clone());
        ticket
    };
    let job = Job {
        ticket: ticket.clone(),
        config: worker.config.clone(),
        network: Arc::clone(&worker.network),
        launcher: Arc::clone(&worker.launcher),
        timer: Arc::clone(&worker.timer),
        shared: Arc::clone(&worker.shared),
        turn: Arc::clone(&worker.job_lock),
        system_proxy: worker.system_proxy.clone(),
        data_lock: Mutex::new(None),
    };
    let spawned = worker
        .shared
        .spawn_job(format!("clauseo-install-{agent}"), move || {
            install::run(&job);
        });
    if let Err(err) = spawned {
        let mut log = InstallLog::new(agent);
        let detail = format!("the install could not start: {err}");
        log.push(redact(&detail, &worker.config.data_dir));
        log.push(format!(
            "result: {} after 0 tries",
            FailureKind::Failed.word()
        ));
        let failure = Failure {
            kind: FailureKind::Failed,
            need_mb: None,
            free_mb: None,
            log,
        };
        worker
            .shared
            .emit_job(&ticket, InstallState::Failed(failure), None, true);
    }
}
