//! The stored API key and the hold (decision record 0048): a key from `with_launch_env` reaches
//! the reader's env under the vendor's variable and never its output, and `hold` keeps an
//! agent's reads off until its guard drops.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clauseo_limits::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
use clauseo_protocol::{AgentKind, Billing};
use futures::FutureExt as _;
use futures::executor::block_on;
use futures::future::join;

use crate::harness::{
    CODEX_JS, FAKE_KEY, FAKE_OPENAI_KEY, Play, Setup, SharedTimer, TestLauncher, under,
};

/// A setup whose settings hold Claude's and Codex's entries as an install writes them: the key
/// variable present and empty (`anthropic_api_key` in another case, to show case is ignored),
/// and no key anywhere in settings.
fn installed_setup() -> Setup {
    let setup = Setup::new();
    Setup::file(&setup.codex_script(), "");
    Setup::file(&under(&setup.install().join("node_modules"), CODEX_JS), "");
    crate::write::create_dir_all(&setup.codex_home()).unwrap();
    let settings = serde_json::json!({"agents": {
        "claude-acp": {
            "command": Setup::command(),
            "args": [setup.script()],
            "env": {"anthropic_api_key": ""},
        },
        "codex-acp": {
            "command": Setup::command(),
            "args": [setup.codex_script()],
            "env": {"CODEX_API_KEY": ""},
        },
    }});
    crate::write::write(&setup.data().join("settings.json"), settings.to_string()).unwrap();
    setup
}

/// The stored keys: `FAKE_KEY` for Claude, `FAKE_OPENAI_KEY` for Codex.
fn stored(agent: AgentKind) -> String {
    match agent {
        AgentKind::Claude => FAKE_KEY.to_owned(),
        AgentKind::Codex => FAKE_OPENAI_KEY.to_owned(),
    }
}

/// The variable a stored key goes in for `agent`, as desktop's hook names it.
fn key_var(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "ANTHROPIC_API_KEY",
        AgentKind::Codex => "CODEX_API_KEY",
    }
}

fn keyed(
    setup: &Setup,
    launcher: &Arc<TestLauncher>,
    timer: &SharedTimer,
) -> ProcessReaders<SharedTimer> {
    setup
        .readers(launcher, timer)
        .with_launch_env(|agent| vec![(key_var(agent).to_owned(), stored(agent))])
}

/// The one spec's env names, and its value of `var`. Never prints a value but `var`'s.
fn names_and(launcher: &TestLauncher, var: &str) -> (Vec<OsString>, Option<OsString>) {
    let specs = launcher.specs();
    let [spec] = specs.as_slice() else {
        panic!("expected one launch, got {}", specs.len());
    };
    let names = spec.env().keys().cloned().collect();
    (names, spec.env().get(OsStr::new(var)).cloned())
}

/// The inherited names a reader keeps (no `GIT_*`, `SSH_*`, `NO_BROWSER` or
/// `CLAUDE_CODE_REMOTE`, nor any name in `dropped`), minus `var` in any case, plus the
/// `extra` names, sorted.
fn expected_names(var: &str, extra: &[&str], dropped: &[&str]) -> Vec<OsString> {
    let kept = |name: &OsString| {
        let bytes = name.as_encoded_bytes();
        let prefixed = ["GIT_", "SSH_"].iter().any(|prefix| {
            bytes
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix.as_bytes()))
        });
        !prefixed
            && !["NO_BROWSER", "CLAUDE_CODE_REMOTE", var]
                .iter()
                .chain(dropped)
                .any(|drop| name.eq_ignore_ascii_case(drop))
            && !extra.iter().any(|e| name == e)
    };
    let mut names: Vec<OsString> = std::env::vars_os()
        .map(|(name, _)| name)
        .filter(kept)
        .chain(extra.iter().map(OsString::from))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The Codex reader's own drops (`readers/env.rs`'s `CODEX_DROPPED`).
const CODEX_DROPPED: [&str; 4] = [
    "CODEX_PATH",
    "APP_SERVER_LOGS",
    "DEFAULT_AUTH_REQUEST",
    "MODEL_PROVIDER",
];

#[test]
fn a_stored_claude_key_replaces_the_empty_placeholder_and_reads_pay_as_you_go() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Claude));
    let ReadOutcome::Usage { limits, .. } = &outcome else {
        panic!("not usage: {outcome:?}");
    };
    assert!(matches!(limits.billing, Billing::PayAsYouGo { .. }));
    let (names, value) = names_and(&launcher, "ANTHROPIC_API_KEY");
    assert_eq!(
        names,
        expected_names("ANTHROPIC_API_KEY", &["ANTHROPIC_API_KEY"], &[])
    );
    assert!(
        value == Some(OsString::from(FAKE_KEY)),
        "not the stored key"
    );
    launcher.wait_stdout_closed();
}

#[test]
fn a_stored_codex_key_goes_in_as_codex_api_key() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("codex-crash"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Codex));
    let ReadOutcome::Failed(ReadFailure::Exited { stderr_tail, .. }) = &outcome else {
        panic!("not an exit: {outcome:?}");
    };
    // codex-crash echoes FAKE_OPENAI_KEY: the stored key alone makes the redactor know it.
    assert!(!stderr_tail.contains(FAKE_OPENAI_KEY), "{stderr_tail}");
    assert!(!format!("{outcome:?}").contains(FAKE_OPENAI_KEY));
    let (names, value) = names_and(&launcher, "CODEX_API_KEY");
    assert_eq!(
        names,
        expected_names(
            "CODEX_API_KEY",
            &["CODEX_API_KEY", "CODEX_HOME"],
            &CODEX_DROPPED
        )
    );
    assert!(
        value == Some(OsString::from(FAKE_OPENAI_KEY)),
        "not the stored key"
    );
    launcher.wait_stdout_closed();
}

#[test]
fn a_crash_never_shows_the_stored_key() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-crash"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let outcome = block_on(reader.read(AgentKind::Claude));
    let ReadOutcome::Failed(ReadFailure::Exited { stderr_tail, .. }) = &outcome else {
        panic!("not an exit: {outcome:?}");
    };
    assert!(stderr_tail.contains("[redacted]"), "{stderr_tail}");
    assert!(!stderr_tail.contains(FAKE_KEY), "{stderr_tail}");
    assert!(!format!("{outcome:?}").contains(FAKE_KEY));
    launcher.wait_stdout_closed();
}

#[test]
fn no_variable_given_leaves_the_settings_variables() {
    for empty in [false, true] {
        let setup = installed_setup();
        let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
        let reader = setup.readers(&launcher, &SharedTimer::default());
        let reader = if empty {
            reader.with_launch_env(|_| Vec::new())
        } else {
            reader
        };
        block_on(reader.read(AgentKind::Claude));
        let (names, value) = names_and(&launcher, "anthropic_api_key");
        assert_eq!(
            names,
            expected_names("ANTHROPIC_API_KEY", &["anthropic_api_key"], &[])
        );
        assert_eq!(value, Some(OsString::new()));
        launcher.wait_stdout_closed();
    }
}

#[test]
fn a_held_agent_launches_nothing_until_its_guard_drops() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let guard = reader
        .hold(AgentKind::Claude)
        .now_or_never()
        .expect("no read ran");
    let mut read = reader.read(AgentKind::Claude);
    assert!((&mut read).now_or_never().is_none());
    assert_eq!(launcher.launches(), 0);
    // Codex is not held.
    assert!(matches!(
        block_on(reader.read(AgentKind::Codex)),
        ReadOutcome::Failed(_)
    ));
    assert_eq!(launcher.launches(), 1);
    drop(guard);
    assert!(matches!(block_on(read), ReadOutcome::Usage { .. }));
    assert_eq!(launcher.launches(), 2);
    launcher.wait_stdout_closed();
    assert!(Path::new(&setup.helper()).is_file());
}

#[test]
fn a_hold_waits_for_the_read_in_flight_to_end() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Hold);
    let timer = SharedTimer::default();
    let reader = keyed(&setup, &launcher, &timer);
    let (outcome, guard) = block_on(join(reader.read(AgentKind::Claude), async {
        assert_eq!(timer.0.next_armed().await, Duration::from_secs(25));
        let mut hold = Box::pin(reader.hold(AgentKind::Claude));
        assert!((&mut hold).now_or_never().is_none(), "a read is in flight");
        assert!(timer.0.fire());
        hold.await
    }));
    assert_eq!(outcome, ReadOutcome::Failed(ReadFailure::TimedOut));
    let mut next = reader.read(AgentKind::Claude);
    assert!((&mut next).now_or_never().is_none(), "held");
    assert_eq!(launcher.launches(), 1);
    drop(guard);
    launcher.wait_stdout_closed();
}

#[test]
fn a_hold_survives_with_entry_and_the_copy_reads_the_new_entry_with_the_key() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let guard = reader
        .hold(AgentKind::Claude)
        .now_or_never()
        .expect("no read ran");
    let mut entry = setup
        .effective()
        .agent("claude-acp")
        .cloned()
        .expect("Claude's entry");
    entry
        .env
        .insert("ANTHROPIC_MODEL".to_owned(), "new-entry".to_owned());
    let rebuilt = reader.with_entry(AgentKind::Claude, Some(entry));
    // The hold taken on the old readers holds the copy.
    let mut read = rebuilt.read(AgentKind::Claude);
    assert!(
        (&mut read).now_or_never().is_none(),
        "held across with_entry"
    );
    assert_eq!(launcher.launches(), 0);
    drop(guard);
    assert!(matches!(block_on(read), ReadOutcome::Usage { .. }));
    let (_names, model) = names_and(&launcher, "ANTHROPIC_MODEL");
    assert_eq!(model, Some(OsString::from("new-entry")));
    let (_names, key) = names_and(&launcher, "ANTHROPIC_API_KEY");
    assert!(
        key == Some(OsString::from(FAKE_KEY)),
        "the key source is kept"
    );
    launcher.wait_stdout_closed();
    // A hold taken on the copy holds the old readers too, and no entry reads as none.
    let guard = rebuilt
        .hold(AgentKind::Claude)
        .now_or_never()
        .expect("no read ran");
    assert!(reader.read(AgentKind::Claude).now_or_never().is_none());
    drop(guard);
    let gone = rebuilt.with_entry(AgentKind::Claude, None);
    assert!(matches!(
        block_on(gone.read(AgentKind::Claude)),
        ReadOutcome::Failed(ReadFailure::LaunchFailed(_))
    ));
}

/// The security review's L7: a read asked for during a hold, on the readers as they were,
/// takes its entry once past the gate, so after a leave it reads the entry written meanwhile.
#[test]
fn a_read_queued_during_a_hold_reads_the_entry_set_before_the_hold_drops() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
    let reader = keyed(&setup, &launcher, &SharedTimer::default());
    let guard = reader
        .hold(AgentKind::Claude)
        .now_or_never()
        .expect("no read ran");
    let mut queued = reader.read(AgentKind::Claude);
    assert!((&mut queued).now_or_never().is_none(), "held");
    let mut entry = setup
        .effective()
        .agent("claude-acp")
        .cloned()
        .expect("Claude's entry");
    entry
        .env
        .insert("ANTHROPIC_MODEL".to_owned(), "after-the-leave".to_owned());
    reader.set_entry(AgentKind::Claude, Some(entry));
    drop(guard);
    assert!(matches!(block_on(queued), ReadOutcome::Usage { .. }));
    let (_names, model) = names_and(&launcher, "ANTHROPIC_MODEL");
    assert_eq!(model, Some(OsString::from("after-the-leave")));
    launcher.wait_stdout_closed();
}

/// The security review's M4: the plan's variables the caller gives go in beside the key, each
/// over a settings variable of that name in any case.
#[test]
fn the_plan_s_variables_go_in_beside_the_key() {
    let setup = installed_setup();
    let launcher = TestLauncher::new(Play::Scenario("claude-api-key"));
    let reader = setup
        .readers(&launcher, &SharedTimer::default())
        .with_launch_env(|agent| {
            vec![
                (key_var(agent).to_owned(), stored(agent)),
                (
                    "ANTHROPIC_BASE_URL".to_owned(),
                    "https://gw.firm.example".to_owned(),
                ),
                ("CLAUDE_CODE_USE_BEDROCK".to_owned(), "1".to_owned()),
            ]
        });
    block_on(reader.read(AgentKind::Claude));
    let (_names, url) = names_and(&launcher, "ANTHROPIC_BASE_URL");
    assert_eq!(url, Some(OsString::from("https://gw.firm.example")));
    let (_names, bedrock) = names_and(&launcher, "CLAUDE_CODE_USE_BEDROCK");
    assert_eq!(bedrock, Some(OsString::from("1")));
    let (_names, key) = names_and(&launcher, "ANTHROPIC_API_KEY");
    assert!(key == Some(OsString::from(FAKE_KEY)), "not the stored key");
    launcher.wait_stdout_closed();
}
