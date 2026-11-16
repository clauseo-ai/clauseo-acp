//! The `SessionStart` settings canary of a Claude session call: armed with the call's `_meta`
//! ([`arm`]), read back after its answer ([`after`]), and judged on the actor ([`passed`]).
//!
//! A `SessionStart` hook in the flag settings (`options.settings`, the `--settings` file) runs,
//! and finishes, before claude-agent-acp 0.89.1 answers `session/new`, `session/load` or
//! `session/resume` (and `new` with `options.resume`), and the answer waits for it (Claude Code
//! 2.1.293). So each such call of a Claude agent carries the canary's entry
//! (`agent::loads::session_canary`): the guard program writes a fresh nonce to a fresh file in
//! the session's run folder (`pre_write_guard/canary.rs`), and after the answer, off the actor,
//! the file must hold exactly that nonce before anything else goes to the session (the mode
//! switch, the picks, a prompt).
//!
//! A canary that is missing, holds anything else, or could not be read (a canary that failed,
//! timed out or never ran reads the same) gets one retry of the same call with a fresh setup:
//! the same session call kind (`session/new` again, or `session/load` or `session/resume` of the
//! same stored session), a fresh `_meta` (a new nonce, a new nonce file, a new settings file,
//! `agent::launch_settings`), and the updates held from the first try dropped (a second load
//! replays the session again; the dedupe view read for the first load is kept). A `session/new`
//! retried leaves the first session in the agent unused until the agent ends. A second failure
//! refuses the launch with `agent.settings_not_loaded` before any prompt (`launch_failed`).
//!
//! Only a Claude agent with the guard program is checked, with `actor.settings_canary` on
//! (`HostConfig::with_settings_canary`). With no guard entry possible (no program, or a program
//! or folder path that is no UTF-8: `agent::loads::guard_missing`), no canary goes, as no guard
//! goes: `profile_loads/policy.rs` `check_guard` has already refused the launches that rest on
//! the guard, and the rest go on unchecked, with a warning. Nor when the firm's managed settings
//! turn hooks off (`disableAllHooks`, `allowManagedHooksOnly`): no hook can run there, so the
//! canary could only refuse every launch; `check_guard` refuses those that rest on the guard.
//!
//! These three calls are every session setup the host sends: Clauseo never sends
//! `session/fork`, and `tests.rs` pins that the only setup calls `clauseo-acp` can send are new,
//! load and resume, all sent from `session.rs`, so a setup call added later cannot skip the
//! canary unnoticed. A fork would be covered too (Claude Code 2.1.293 with claude-agent-acp
//! 0.89.1): Claude Code runs no `SessionStart` hook at `session/fork`, a forked session cannot
//! be prompted until a `session/resume` of its id, and that resume carries the only settings
//! the forked CLI gets and fires `SessionStart` (source `resume`). So the fork's canary is its
//! resume's: every resume, as every load and new, carries the full flag settings
//! (`profile_loads::session_loads`: the guard's entry and this canary's) and is checked here.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, MatterId, ThreadId};
use serde_json::{Map, Value};

use super::super::super::Actor;
use super::super::agents::{Stage, launch_failed};
use super::super::state;
use crate::agent::codex::prepare::{Job, off_actor};
use crate::agent::loads::session_canary::{CanaryHook, add_canary, canary_entry};
use crate::clock::Clock;
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// The nonce file's name in the run folder before its nonce.
const FILE_PREFIX: &str = "session-canary.";

/// The nonce's random bytes (32 hex digits).
const NONCE_BYTES: usize = 16;

/// The most of a nonce file read back: more than any nonce, so a longer file reads as another.
const READ_CAP: u64 = 256;

/// Which session call a canary went with, for its one retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Call {
    New,
    Load,
    Resume,
}

/// What one session call's canary must leave: its file and nonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Expect {
    file: PathBuf,
    nonce: String,
}

/// What the read-back found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Found {
    /// The file holds exactly the nonce: the settings loaded.
    Written,
    /// No file: the canary never ran, failed or timed out.
    Missing,
    /// A file with anything else in it.
    Other,
    /// The file could not be read, or the read did not answer in time.
    Unread,
}

/// A session call's canary, read back after a good answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checked {
    call: Call,
    found: Found,
}

/// How a session call answered well, for `session.rs` `on_started`: `true` when it continued the
/// stored session (load, resume), and its canary read back, when it carried one.
pub(crate) type Started = (bool, Option<Checked>);

/// A Claude agent's canary state (`Agent::canary`).
#[derive(Debug, Default)]
pub(crate) struct Canary {
    /// The canary [`arm`] put in the `_meta` of the session call about to go, which takes it.
    armed: Option<Expect>,
    /// Whether this launch already retried its session setup for a canary.
    retried: bool,
}

/// Puts a fresh canary's entry in `meta`, the `_meta` of `thread`'s next session call, and
/// keeps what it must write on the agent, when the launch is checked (the module doc): the
/// switch is on, the agent is Claude's (`claude`), the guard program gives an entry, and the
/// firm's settings do not turn hooks off (`hooks_off`). `root` is the session's run folder.
/// Fails only when no random bytes could be had for the nonce.
pub(in super::super) fn arm(
    actor: &mut Actor,
    thread: ThreadId,
    meta: &mut Map<String, Value>,
    (root, claude, hooks_off): (&Path, bool, Option<&'static str>),
) -> Result<(), Error> {
    let on = actor.settings_canary && claude;
    let program = actor.pre_write_guard.clone();
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return Ok(());
    };
    agent.canary.armed = None;
    if !on {
        return Ok(());
    }
    if let Some(key) = hooks_off {
        tracing::warn!(
            %thread,
            key,
            "the firm's Claude settings turn hooks off: no settings canary can run, so the \
             session's settings are not checked"
        );
        return Ok(());
    }
    let nonce = nonce()?;
    let file = root.join(format!("{FILE_PREFIX}{nonce}"));
    let entry = program.as_deref().and_then(|program| {
        canary_entry(CanaryHook {
            program,
            file: &file,
            nonce: &nonce,
        })
    });
    let Some(entry) = entry else {
        let reason = if program.is_none() {
            "no_program"
        } else {
            "path_not_utf8"
        };
        tracing::warn!(
            %thread,
            reason,
            "no settings canary for this Claude session: its settings are not checked"
        );
        return Ok(());
    };
    add_canary(meta, entry);
    agent.canary.armed = Some(Expect { file, nonce });
    Ok(())
}

/// A new nonce: [`NONCE_BYTES`] random bytes as lowercase hex.
fn nonce() -> Result<String, Error> {
    let mut bytes = [0u8; NONCE_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        tracing::warn!(%error, "no random bytes for the settings canary");
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            "Clauseo could not make the check of the agent's settings.",
        )
    })?;
    let mut hex = String::with_capacity(NONCE_BYTES * 2);
    for byte in bytes {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
    }
    Ok(hex)
}

/// Takes the canary [`arm`] armed for `thread`'s session call, which carries its entry.
pub(super) fn take(actor: &mut Actor, thread: ThreadId) -> Option<Expect> {
    state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .and_then(|agent| agent.canary.armed.take())
}

/// How long the read-back's caller waits. 10 s, not the 5 s of `Job::short`, which the turn's
/// cancel ladder also arms (a test moving the clock past a 5 s timer means the cancel's).
const READ_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// After a session call (`call`) for `matter` answered (`answered`, well): its canary `expect`
/// read back off the actor (`crate::agent::codex::prepare::off_actor`, waited on for
/// [`READ_WAIT`], so a read that hangs reads as [`Found::Unread`]). `None` when it carried no
/// canary or failed.
pub(super) async fn after(
    clock: Arc<dyn Clock>,
    matter: Option<MatterId>,
    (call, expect): (Call, Option<Expect>),
    answered: bool,
) -> Option<Checked> {
    let expect = expect.filter(|_| answered)?;
    let job = Job {
        what: "the settings canary's read-back",
        matter,
        wait: READ_WAIT,
    };
    let found = off_actor(clock, job, move || Ok(read(&expect)))
        .await
        .unwrap_or(Found::Unread);
    Some(Checked { call, found })
}

/// Whether `expect`'s file holds exactly its nonce. Opened through no link at its name.
fn read(expect: &Expect) -> Found {
    let file = match crate::write::open_read_nofollow(&expect.file) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Found::Missing,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "the settings canary's file cannot be read");
            return Found::Unread;
        }
    };
    let mut bytes = Vec::new();
    match file.take(READ_CAP).read_to_end(&mut bytes) {
        Ok(_) if bytes == expect.nonce.as_bytes() => Found::Written,
        Ok(_) => Found::Other,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "the settings canary's file cannot be read");
            Found::Unread
        }
    }
}

/// A session call answered well: `true` when it carried no canary, or its canary wrote its
/// nonce, and the session goes on. Otherwise the session setup is sent once more with a fresh
/// canary (the module doc), or, when it already was, the launch is refused with
/// `agent.settings_not_loaded`; `false` either way, and for an agent no longer starting.
pub(super) fn passed(actor: &mut Actor, thread: ThreadId, checked: Option<Checked>) -> bool {
    // An agent stopped while its session call ran (the lawyer's Stop of its launch, `cancel.rs`
    // `abandon_launch`) goes no further: its late answer attaches nothing.
    let loading = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| agent.stage == Stage::Loading);
    if !loading {
        return false;
    }
    let Some(Checked { call, found }) = checked else {
        return true;
    };
    if found == Found::Written {
        tracing::info!(target: LIFECYCLE, %thread, ?call, "Claude's settings loaded");
        return true;
    }
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return false;
    };
    if !agent.canary.retried {
        agent.canary.retried = true;
        // The first try's updates (a load's replay) belong to a setup that is dropped.
        agent.replayed.clear();
        tracing::warn!(
            %thread,
            ?call,
            ?found,
            "Claude's settings canary wrote no nonce: the session setup is sent once more"
        );
        match call {
            Call::New => super::new_session(actor, thread),
            Call::Load => super::send_load(actor, thread, None),
            Call::Resume => super::resume_session(actor, thread),
        }
        return false;
    }
    tracing::warn!(
        %thread,
        ?call,
        ?found,
        "Claude's settings canary wrote no nonce twice: the launch is refused"
    );
    let error = Error::refused(
        ErrorCode::AgentSettingsNotLoaded,
        "Claude Code did not load Clauseo's settings for this session (their check never ran), \
         so Clauseo sent no message and stopped Claude",
    );
    launch_failed(actor, thread, &error);
    false
}

#[cfg(test)]
mod tests;
