//! A session's run folder removal ([`discard`]): off the actor, it waits for the session's
//! processes ([`super::straggler`]), then frees the run's claims, relay and proxy and removes
//! its folder, or holds them until the host stops.

use clauseo_process::TreeWatch;
use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::TurnInput;
use super::{RunFolders, relay, straggler};
use crate::agent::codex;
use crate::config::SessionKey;
use crate::error::Error;
use crate::log_file::Cause;

/// Removes `run` (`thread`'s launch's run folder) off the actor and frees its browser claims,
/// relay and proxy there; the notes folder stays. The session has ended, or never started a
/// process (`tree` none). First, on that thread, the removal waits until the agent's tree
/// (`tree`) and the session's Chrome are gone, up to `HostConfig::with_tree_wait`
/// ([`straggler`]); only then is the run's lease marked done, so its parts are freed as it
/// drops. Past the cap, or on any other way out (the thread did not start, a panic), the
/// lease holds them until the host stops and the folder stays. The session's browser slot is
/// released only once the run was freed ([`on_run_removed`]): a held run keeps it, so no new
/// Chrome starts while a held one may live. The removal counts as work in flight
/// (`Work::run_removals`) until its [`TurnInput::RunRemoved`], so shutdown
/// answers only once it is done.
pub(super) fn discard(
    actor: &mut Actor,
    thread: ThreadId,
    run: RunFolders,
    tree: Option<TreeWatch>,
) {
    let key = run.key;
    let clock = std::sync::Arc::clone(&actor.clock);
    let cap = actor.wiring.tree_wait;
    let job = codex::prepare::Job {
        what: "run folder removal",
        matter: Some(key.matter),
        wait: cap.saturating_add(codex::prepare::PREPARE_WAIT),
    };
    actor.work.run_removals += 1;
    let hook = actor.wiring.run_removal_hook.clone();
    let held = actor.wiring.held.clone();
    let wait_hook = actor.wiring.tree_wait_hook.clone();
    let thread_fails = actor.wiring.removal_thread_fails;
    actor.spawn_post(async move {
        let work = move || {
            let browser = run.browser_session.as_deref();
            if let Some(wait_hook) = &wait_hook {
                wait_hook(browser.map(|session| session.ports.cdp.port()));
            }
            let stopping = || held.stopping();
            if let Err(straggler) = straggler::wait_gone(tree.as_ref(), browser, cap, stopping) {
                // After the host's stop has released the held runs, this one is freed at once.
                let after_stop = held.closed();
                tracing::warn!(
                    matter = %key.matter,
                    thread = %key.thread,
                    launch = key.launch,
                    cap_ms = u64::try_from(cap.as_millis()).unwrap_or(u64::MAX),
                    still_there = straggler.as_str(),
                    after_stop,
                    "a session's processes outlived the wait; its proxy, relay, ports and run \
                     folder are held until the host stops"
                );
                // The lease is not done: dropping the run holds its parts.
                drop(run);
                return Ok(false);
            }
            run.lease.wait_done();
            // The token is revoked before its file goes with the folder.
            relay::end(&run);
            if let Some(hook) = &hook {
                hook(&run.root);
            }
            run.remove();
            // The browser claims, the case-law relay and the proxy go here, off the actor and
            // once the agent's tree and Chrome are gone: the socket folder is removed, the relay
            // stops listening, the proxy closes and the ports are free for the next session.
            drop(run);
            Ok(true)
        };
        let removed = if thread_fails {
            // As `off_actor` does when its thread does not start: the work drops here.
            drop(work);
            Err(Error::refused(
                clauseo_protocol::ErrorCode::AgentLaunchFailed,
                "the removal's thread did not start (a test's seam)",
            ))
        } else {
            codex::prepare::off_actor(clock, job, work).await
        };
        let freed = match removed {
            Ok(freed) => freed,
            Err(error) => {
                tracing::warn!(
                    %thread,
                    error = %Cause(&error),
                    "a session's run folder was not removed; its parts are held until the host stops"
                );
                false
            }
        };
        Input::Turn(TurnInput::RunRemoved { key, freed })
    });
}

/// A run folder's removal ended: shutdown no longer waits for it, and a session whose run was
/// freed (`freed`) gives back its browser slot. A held run keeps its slot until the host stops.
pub(in super::super) fn on_run_removed(actor: &mut Actor, key: SessionKey, freed: bool) {
    actor.work.run_removals = actor.work.run_removals.saturating_sub(1);
    if freed {
        actor.wiring.browser_slots.release(key);
    }
}
