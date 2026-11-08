//! Fixes to the start's `Ready` from a receipt: an install pressed while the check decides an
//! agent waits for its answer; the check's receipt rewrite takes `agents/.lock`; a receipt
//! missing a path the launch needs is `Failed` at once; and a `PATH` Node changed since the
//! receipt was written waits for the check.

use std::sync::Arc;
use std::time::Duration;

use clauseo_agents::{Agents, FailureKind, InstallState, Installed, Method, SigninState};

use crate::receipt_ready::{
    BOUND, Events, Gate, Mode, drain_within, install_first, next_for, start_gated, until_within,
};
use crate::receipt_signin::{claude, claude_fixture, signin_until};
use crate::support::{Fixture, agent_a};
use crate::write;

/// Rewrites the receipt of `installed` through `edit`.
fn edit_receipt(installed: &Installed, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = installed.folder.join("clauseo-receipt.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut receipt);
    write::write(&path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

/// The receipt of `installed`, as JSON.
fn receipt_of(installed: &Installed) -> serde_json::Value {
    let path = installed.folder.join("clauseo-receipt.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// Drops the receipt's Node stamp, as a receipt written before it holds: the start cannot tell
/// the Node is unchanged, so the agent waits for the check.
fn unstamp(installed: &Installed) {
    edit_receipt(installed, |receipt| {
        receipt.as_object_mut().unwrap().remove("node_stamp");
    });
}

/// Claude, installed with an unstamped receipt, started with only the check's
/// `node --version` held; an install and a sign-in are pressed while it is, and the worker has
/// taken both (the sign-in's `Queued`, sent after the install, shows it).
fn press_during_check(label: &str) -> (Fixture, Installed, Started) {
    let fx = claude_fixture(label);
    let installed = install_first(&fx);
    unstamp(&installed);
    let gate = Gate::hold_first();
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();
    assert_eq!(
        agents.state(&claude()),
        None,
        "undecided while the check runs"
    );
    agents.install(&claude());
    agents.sign_in(&claude(), Method::ClaudeAi).unwrap();
    let queued = signin_until(&events, |state| matches!(state, SigninState::Queued));
    assert_eq!(queued, SigninState::Queued);
    // The press waits: no install job runs meanwhile (one would send `Ready` within this).
    let quiet = events_for(&events, QUIET);
    assert_eq!(quiet, Vec::new(), "nothing for Claude while its check runs");
    (fx, installed, (agents, events, gate))
}

/// How long a press during the check is watched for an install job's events.
const QUIET: Duration = Duration::from_secs(3);

/// Claude's install events within `window`.
fn events_for(events: &Events, window: Duration) -> Vec<InstallState> {
    let mut seen = Vec::new();
    while let Some((agent, state)) = next_for(events, window) {
        if agent == claude() {
            seen.push(state);
        }
    }
    seen
}

/// A start under a gate: the handle, its events and the gate.
type Started = (Agents, Events, Arc<Gate>);

/// Claude's install events from now until the shutdown, in order.
fn claude_events(events: &Events) -> Vec<InstallState> {
    drain_within(events)
        .into_iter()
        .filter(|(agent, _)| *agent == claude())
        .map(|(_, state)| state)
        .collect()
}

#[test]
fn an_install_pressed_during_a_check_that_passes_starts_no_job_and_signs_in_at_ready() {
    let (_fx, installed, (agents, events, gate)) = press_during_check("press-pass");

    gate.release(false);
    let ready = until_within(&events, &claude(), |_| true);
    assert!(matches!(ready, InstallState::Ready { .. }), "{ready:?}");
    let started = signin_until(&events, |state| !matches!(state, SigninState::Queued));
    assert!(!matches!(started, SigninState::Queued), "{started:?}");
    assert!(agents.shutdown(BOUND), "every thread ended");
    // No install job ran: none ever ended to record its tries.
    assert_eq!(agents.install_tries(&claude()), None);
    assert_eq!(agents.installed(&claude()), Some(installed));
    assert_eq!(claude_events(&events), Vec::<InstallState>::new());
}

#[test]
fn an_install_pressed_during_a_check_that_fails_installs_once_after_it() {
    let (_fx, installed, (agents, events, gate)) = press_during_check("press-fail");

    gate.release(true);
    // The check's answer comes first; the press's one install only after it.
    let first = until_within(&events, &claude(), |_| true);
    assert!(matches!(first, InstallState::Missing { .. }), "{first:?}");
    let last = until_within(&events, &claude(), |state| {
        matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
    });
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let started = signin_until(&events, |state| !matches!(state, SigninState::Queued));
    assert!(!matches!(started, SigninState::Queued), "{started:?}");
    assert!(agents.shutdown(BOUND), "every thread ended");
    assert_eq!(
        agents.install_tries(&claude()),
        Some(0),
        "one job, finding the install"
    );
    assert_eq!(agents.installed(&claude()), Some(installed));
    assert_eq!(claude_events(&events), Vec::<InstallState>::new());
}

#[test]
fn the_checks_receipt_rewrite_waits_for_agents_lock() {
    let fx = Fixture::new("rewrite-lock");
    let installed = install_first(&fx);
    unstamp(&installed);
    let before = std::fs::read(installed.folder.join("clauseo-receipt.json")).unwrap();

    // Another Clauseo installs: the check decides the agent but leaves its receipt alone.
    let held = write::hold_lock(&fx.data.path().join("agents").join(".lock")).unwrap();
    let gate = Gate::new(Mode::Pass);
    let (agents, events) = start_gated(&fx, &gate);
    let ready = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(ready, InstallState::Ready { .. }), "{ready:?}");
    assert!(agents.shutdown(BOUND), "the check ended");
    drop(agents);
    let after = std::fs::read(installed.folder.join("clauseo-receipt.json")).unwrap();
    assert_eq!(after, before, "no rewrite under another's lock");

    // With the lock free, the check stamps the receipt, so the next start is Ready at once.
    drop(held);
    let (agents, events) = start_gated(&fx, &gate);
    until_within(&events, &agent_a(), |_| true);
    assert!(agents.shutdown(BOUND), "the check ended");
    assert!(receipt_of(&installed).get("node_stamp").is_some());
}

#[test]
fn a_path_node_changed_since_the_receipt_waits_for_the_check() {
    let fx = Fixture::new("node-changed");
    let installed = install_first(&fx);
    edit_receipt(&installed, |receipt| {
        receipt["node_stamp"]["size"] = 1.into();
    });
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();
    // The worker's first states are out before the check starts: none for A.
    assert_eq!(agents.state(&agent_a()), None, "no Ready before the check");

    gate.set(Mode::Pass);
    let ready = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(ready, InstallState::Ready { .. }), "{ready:?}");
    assert!(agents.shutdown(BOUND), "the check ended");
    assert_eq!(agents.installed(&agent_a()), Some(installed));
}

/// The `Failed` a missing path gives at start, checked; the path's line is returned.
fn missing_at_start(fx: &Fixture) -> (Agents, Events, String) {
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(fx, &gate);
    let first = until_within(&events, &agent_a(), |_| true);
    let InstallState::Failed(failure) = first else {
        panic!("{first:?}");
    };
    assert_eq!(failure.kind, FailureKind::SetupFailed);
    let lines = failure.log.lines();
    assert_eq!(
        lines.last().map(String::as_str),
        Some("result: setup_failed (set-up needs repair: install files missing)")
    );
    assert!(
        !lines.iter().any(|line| line.contains("tries")),
        "{lines:?}"
    );
    let line = lines
        .iter()
        .find(|line| line.starts_with("a file this agent's install needs is missing:"))
        .cloned()
        .unwrap_or_else(|| panic!("{lines:?}"));
    gate.set(Mode::Pass);
    (agents, events, line)
}

#[test]
fn a_missing_entry_script_fails_at_start_and_retry_repairs_it() {
    let fx = Fixture::new("missing-script");
    let installed = install_first(&fx);
    write::remove(&installed.script).unwrap();

    let (agents, events, line) = missing_at_start(&fx);
    assert!(
        line.ends_with("node_modules/adapter-a/dist/index.js"),
        "{line}"
    );
    agents.retry(&agent_a(), false);
    let last = until_within(&events, &agent_a(), |state| {
        matches!(state, InstallState::Ready { .. } | InstallState::Failed(_))
    });
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    assert!(installed.script.is_file(), "the install was redone");
    assert!(agents.shutdown(BOUND), "every thread ended");
}

#[test]
fn a_missing_module_folder_fails_at_start() {
    let fx = Fixture::new("missing-module");
    let installed = install_first(&fx);
    let files = receipt_of(&installed)["files"].clone();
    assert!(
        files
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "node_modules/dep-a"),
        "{files}"
    );
    write::remove(&installed.folder.join("node_modules").join("dep-a")).unwrap();

    let (agents, _events, line) = missing_at_start(&fx);
    assert!(line.ends_with("node_modules/dep-a"), "{line}");
    assert!(agents.shutdown(BOUND), "every thread ended");
}

#[test]
fn an_old_receipt_without_the_list_is_ready_while_its_script_is_there() {
    let fx = Fixture::new("old-receipt");
    let installed = install_first(&fx);
    edit_receipt(&installed, |receipt| {
        receipt.as_object_mut().unwrap().remove("files");
    });
    let gate = Gate::new(Mode::Hold);
    let (agents, events) = start_gated(&fx, &gate);
    gate.wait_held();
    let first = until_within(&events, &agent_a(), |_| true);
    assert!(matches!(first, InstallState::Ready { .. }), "{first:?}");
    gate.set(Mode::Pass);
    assert!(agents.shutdown(BOUND), "the check ended");
    drop(agents);

    // Its script, derived from the snapshot, is still checked.
    write::remove(&installed.script).unwrap();
    let (agents, _events, line) = missing_at_start(&fx);
    assert!(
        line.ends_with("node_modules/adapter-a/dist/index.js"),
        "{line}"
    );
    assert!(agents.shutdown(BOUND), "every thread ended");
}
