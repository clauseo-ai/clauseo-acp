//! The setup pump's thread: `Agents`' events pumped while the setup thread waits for the keys.

use std::sync::{Arc, Mutex};

use clauseo_agents::AgentsEvent;
use futures::Stream;

use crate::bridge::lock;
use crate::chooser::trace::at_ms;
use crate::setup::backend::AgentsBackend;
use crate::setup::status_timing::TARGET;

/// The pump of `Agents`' events: on its own thread, started before the keys' load ends, or,
/// when that thread could not start, the events kept for this one once the load is over.
pub(super) enum Pump<S> {
    Thread(std::thread::JoinHandle<()>),
    Here(Option<S>),
}

impl<S: Stream<Item = AgentsEvent>> Pump<S> {
    /// Waits until the pump ends (`Agents` has ended), running it here if it has no thread.
    pub(super) fn finish(self, backend: &AgentsBackend) {
        match self {
            Pump::Thread(thread) => {
                if thread.join().is_err() {
                    tracing::warn!("the setup pump panicked");
                }
            }
            Pump::Here(Some(source)) => futures::executor::block_on(backend.pump(source)),
            Pump::Here(None) => {}
        }
    }
}

/// Starts `backend`'s pump of `source` on a `clauseo-setup-pump` thread, so it runs while
/// this thread waits for the keys: an install's `Ready` asks for the status check at once, while
/// the sign-in states it sends wait for the load in the pump (`AgentsBackend::pump`). The
/// launches stay refused until the load is over.
pub(super) fn start_pump<S>(backend: &Arc<AgentsBackend>, source: S) -> Pump<S>
where
    S: Stream<Item = AgentsEvent> + Send + 'static,
{
    tracing::info!(target: TARGET, at_ms = at_ms(), "agents started");
    let slot = Arc::new(Mutex::new(Some(source)));
    let (pumping, taken) = (Arc::clone(backend), Arc::clone(&slot));
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-pump".to_owned())
        .spawn(move || {
            let source = lock(&taken).take();
            if let Some(source) = source {
                futures::executor::block_on(pumping.pump(source));
            }
        });
    match spawned {
        Ok(thread) => Pump::Thread(thread),
        Err(err) => {
            tracing::warn!(%err, "no pump thread: the agents' events wait for the keys' load");
            // A thread that did not start never took the events.
            Pump::Here(lock(&slot).take())
        }
    }
}
