//! The session's browser, started over its `DevTools` pipe (route 2, decision record 0060).
//!
//! macOS and Linux: the host's Chrome (`AGENT_BROWSER_EXECUTABLE_PATH`), as before, only over the
//! pipe. Windows: `browsers.rs`'s order, read again at each start with the firm's policy, the
//! browser that started last first; each one tried that fails is killed, and the next starts on a
//! fresh profile (one browser's profile is never handed to another's). When none is left to try,
//! the failure carries the reason the agent is told ([`browsers::Unavailable`]); when each one
//! tried failed, it is the plain "could not start".
//!
//! Each browser is launched (`chrome/pipe.rs`; on Windows hardened and in the job), its record
//! written, on Unix its pid told to the reaper before the wait, and then given the start wait to
//! answer `Browser.getVersion` on the pipe.

#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(any(windows, test))]
use super::super::browsers;
use super::super::chrome::{self, Chrome, Spec};
use super::super::parent::Parent;
use super::super::record::{self, ChromeState};
use super::super::{PARENT_GONE, note};
use super::{Shared, StartError, StartFailure, start_wait};

/// The first browser that answers on its pipe, and the relay's endpoint for agent-browser.
pub(super) fn start_browser(
    shared: &Arc<Shared>,
    parent: &Parent,
) -> Result<(Chrome, String), StartFailure> {
    let launcher = &shared.launcher;
    let candidates = candidates(shared)?;
    let (started, executable) = first_to_start(&candidates, |tried, executable| {
        if tried > 0 {
            chrome::remove_profile(&launcher.profile);
        }
        start_one(shared, executable, parent)
    })?;
    *lock(&launcher.started_with) = Some(executable);
    Ok(started)
}

/// The fallback loop over `candidates`, in order: `start` is given each one's place in the
/// list (a place past 0 starts on a fresh profile) and its path. The first that starts wins;
/// the parent's going, or Chrome's sandbox that cannot start, ends the loop at once; when each one
/// fails, the failure is the only reason, or every reason joined.
fn first_to_start<T>(
    candidates: &[PathBuf],
    mut start: impl FnMut(usize, &PathBuf) -> Result<T, StartError>,
) -> Result<(T, PathBuf), StartFailure> {
    let mut failures = Vec::new();
    for (tried, executable) in candidates.iter().enumerate() {
        note(&format!(
            "starting {} over a DevTools pipe",
            executable.display()
        ));
        match start(tried, executable) {
            Ok(started) => {
                note(&format!(
                    "{} answers on its DevTools pipe; the relay listens on 127.0.0.1",
                    executable.display()
                ));
                return Ok((started, executable.clone()));
            }
            // Chrome's sandbox could not start: refused, and no other browser is tried
            // (`chrome/sandbox.rs`); nor any once the guard's parent has gone.
            Err(refused @ StartError::Sandbox(_)) => return Err(refused.into()),
            Err(StartError::Other(reason)) if reason == PARENT_GONE => return Err(reason.into()),
            Err(StartError::Other(reason)) => {
                note(&format!("{} did not start: {reason}", executable.display()));
                failures.push(reason);
            }
        }
    }
    let [only] = failures.as_slice() else {
        return Err(format!("no browser started: {}", failures.join("; ")).into());
    };
    Err(only.clone().into())
}

/// The browsers to try, in order.
#[cfg(unix)]
#[expect(clippy::unnecessary_wraps, reason = "Windows' version can fail")]
fn candidates(shared: &Shared) -> Result<Vec<PathBuf>, StartFailure> {
    Ok(vec![shared.launcher.executable.clone()])
}

/// The browsers to try, in order: `browsers.rs`'s, the one that started last first.
#[cfg(windows)]
fn candidates(shared: &Shared) -> Result<Vec<PathBuf>, StartFailure> {
    let launcher = &shared.launcher;
    let last = lock(&launcher.started_with).clone();
    plan(browsers::system(&launcher.executable), last.as_deref())
}

/// Windows' list from `choice`, with `last` (the browser that started last) first, each one
/// set aside noted; or, when none is left, the failure with the agent's words.
#[cfg(any(windows, test))]
fn plan(mut choice: browsers::Choice, last: Option<&Path>) -> Result<Vec<PathBuf>, StartFailure> {
    if let Some(last) = last {
        choice.prefer(last);
    }
    if let Some(refused) = &choice.refused {
        note(&format!(
            "{} is not under Program Files; not tried",
            refused.display()
        ));
    }
    for blocked in &choice.blocked {
        note(&format!(
            "{} is blocked by the firm's policy ({}=0 or {}={}); not tried",
            blocked.display(),
            browsers::POLICY_VALUE,
            browsers::DEVTOOLS_VALUE,
            browsers::DEVTOOLS_DISALLOWED
        ));
    }
    if let Some(per_user) = &choice.per_user {
        note(&format!(
            "{} is a per-user install; not tried",
            per_user.display()
        ));
    }
    match choice.unavailable() {
        Some(none) => Err(StartFailure {
            reason: StartError::Other(none.reason().to_owned()),
            agent_note: Some(none.agent_note()),
        }),
        None => Ok(choice.try_in_order),
    }
}

/// Starts the browser at `executable` and waits for its answer. The run's record says
/// `starting`, then the browser's process, and `none` again if it fails (its `Chrome` drop
/// killed it).
fn start_one(
    shared: &Arc<Shared>,
    executable: &PathBuf,
    parent: &Parent,
) -> Result<(Chrome, String), StartError> {
    let launcher = &shared.launcher;
    let spec = Spec {
        executable,
        cwd: &launcher.cwd,
        downloads: &launcher.downloads,
        profile: &launcher.profile,
        proxy: launcher.proxy.as_deref(),
        env: &launcher.chrome_env,
    };
    record::write_state(&launcher.cwd, &launcher.run, &ChromeState::Starting);
    let started = chrome::launch(
        &spec,
        #[cfg(windows)]
        &launcher.job,
    )
    .and_then(|mut browser| {
        record::write_state(&launcher.cwd, &launcher.run, &browser.recorded());
        // Before the wait for the answer: a guard killed during it leaves the reaper to end
        // Chrome's group. Once the guard reaps Chrome, the reaper leaves its pid alone.
        #[cfg(unix)]
        {
            launcher.reaper.say(&format!("chrome {}", browser.pid()));
            let teller = launcher.reaper.clone();
            browser.on_reap(move || teller.say("reaped chrome"));
        }
        note(&format!(
            "{} started; waiting for its answer on its DevTools pipe",
            executable.display()
        ));
        let endpoint = browser.endpoint(
            start_wait(),
            parent,
            (Arc::clone(&shared.held), Arc::clone(&shared.saved)),
        )?;
        // The guard's own CDP commands (PR-3's proxy answer), before agent-browser exists.
        browser.start_guard(&launcher.env)?;
        Ok((browser, endpoint))
    });
    if started.is_err() {
        record::write_state(&launcher.cwd, &launcher.run, &ChromeState::None);
    }
    started
}

fn lock(mutex: &Mutex<Option<PathBuf>>) -> MutexGuard<'_, Option<PathBuf>> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
