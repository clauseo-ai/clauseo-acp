//! A forgotten matter's sandbox state (R17 PR-E): once `ForgetMatter` is durable, the set-up
//! drops the matter's permission entries and its pool user's claim
//! (`SandboxSetup::forget_matter`), so the pool user can go to another matter. It reads and
//! writes the sandbox's records and the matter's folders, so it runs on a thread of its own
//! (`clauseo-sandbox-forget`), never the actor's.
//!
//! - Nothing waits for it: `ForgetMatter` has answered by then, and the actor holds nothing
//!   for it, so there is nothing to bound. Its end is logged with the matter id only, and a
//!   failure by its kind only (its text may name a path or an account, 0047).
//! - A forget that leaves something behind is not retried here: the matter's entries stay in
//!   the sandbox's ledger, and the pool user's reuse rule keeps it from another matter until
//!   a later forget or set-up clears them.
//! - With no set-up (macOS, Linux, no seam) there is nothing to forget.

use std::sync::Arc;

use clauseo_protocol::MatterId;
use clauseo_sandbox::{SandboxSetup, SetupError};

use crate::actor::Actor;

/// The thread's name.
const THREAD_NAME: &str = "clauseo-sandbox-forget";

/// `matter` was forgotten (its commit is durable): the set-up forgets it, off the actor.
pub(in crate::actor) fn matter_forgotten(actor: &Actor, matter: MatterId) {
    let Some(setup) = actor
        .protection
        .sandbox
        .as_ref()
        .and_then(|sandbox| sandbox.setup.clone())
    else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || forget(&setup, matter));
    if let Err(error) = spawned {
        tracing::warn!(
            %matter,
            error = %crate::log_file::Cause(&error),
            "the sandbox's forget of a matter did not start"
        );
    }
}

/// The forget itself, on its thread, logged.
fn forget(setup: &Arc<dyn SandboxSetup>, matter: MatterId) {
    match setup.forget_matter(&matter) {
        Ok(()) => tracing::info!(%matter, "the sandbox forgot a matter"),
        Err(error) => tracing::warn!(
            %matter,
            kind = kind(&error),
            "the sandbox's forget of a matter failed"
        ),
    }
}

/// A set-up error's kind, for the log: never its text.
const fn kind(error: &SetupError) -> &'static str {
    match error {
        SetupError::Refused => "refused",
        SetupError::Failed(_) => "failed",
        SetupError::Blocked(_) => "blocked",
    }
}
