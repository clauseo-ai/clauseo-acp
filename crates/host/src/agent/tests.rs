use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{DetachReason, Timestamp};
use clauseo_settings::AgentLaunch;
use futures::StreamExt;
use futures::channel::mpsc;
use futures::executor::LocalPool;
use futures::future::{pending, ready};
use futures::task::LocalSpawnExt;

use super::{AgentInput, AgentLink, Ended, PUMP_END_BOUND, StopStep, launch_spec, settle, start};
use crate::clock::ManualClock;

/// A 20-character fake GitHub token, as the test's "inherited" environment holds it.
const FAKE_GITHUB_TOKEN: &str = "ghp_fakeToken0123456";
/// A fake Gemini key, also echoed by `stderr-secrets.jsonl`.
const FAKE_GEMINI_KEY: &str = "AIzaFakeGeminiKey01234";
/// A fake SMTP secret, also echoed by `stderr-secrets.jsonl`.
const FAKE_SMTP_SECRET: &str = "smtpFakeSecret98765";
/// A fake database password, also echoed by `stderr-secrets.jsonl`.
const FAKE_DB_PASSWORD: &str = "dbFakePassword45678";

fn scenario(name: &str) -> String {
    let local =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/scenarios/{name}.jsonl"));
    if local.is_file() {
        local.to_string_lossy().into_owned()
    } else {
        name.to_owned()
    }
}

fn fake_agent(scenario_arg: String) -> AgentLaunch {
    let bin = clauseo_fake_agent::bin_path().expect("run cargo build -p clauseo-fake_agent first");
    AgentLaunch {
        command: bin.to_string_lossy().into_owned(),
        args: vec!["--scenario".to_owned(), scenario_arg],
        env: BTreeMap::new(),
    }
}

/// Starts the agent on a fresh pool and returns the pool, the link, the clock and the inputs.
fn run_agent(
    agent: &AgentLaunch,
    inherited: Vec<(OsString, OsString)>,
) -> (
    LocalPool,
    AgentLink,
    ManualClock,
    mpsc::UnboundedReceiver<AgentInput>,
) {
    let spec = launch_spec(
        agent,
        BTreeMap::new(),
        inherited.into_iter(),
        std::env::temp_dir(),
    )
    .unwrap();
    let pool = LocalPool::new();
    let clock = ManualClock::new(Timestamp::ZERO);
    let (tx, rx) = mpsc::unbounded();
    let post: Rc<dyn Fn(AgentInput)> = Rc::new(move |input| tx.unbounded_send(input).unwrap());
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let link = start(
        &pool.spawner(),
        &launcher,
        &spec,
        Arc::new(clock.clone()),
        post,
    )
    .unwrap();
    (pool, link, clock, rx)
}

async fn ended(rx: &mut mpsc::UnboundedReceiver<AgentInput>) -> Ended {
    loop {
        match rx
            .next()
            .await
            .expect("the agent's tasks ended without posting Ended")
        {
            AgentInput::Ended(ended) => return ended,
            AgentInput::Event(_) => {}
        }
    }
}

/// An absolute program path on every OS (`/bin/agent` is not absolute on Windows); the spec
/// checks only that it is absolute, and nothing launches it.
fn absolute_program() -> String {
    std::env::temp_dir()
        .join("agent")
        .to_str()
        .unwrap()
        .to_owned()
}

#[test]
fn launch_spec_layers_inherited_then_agent_then_hook() {
    let agent = AgentLaunch {
        command: absolute_program(),
        args: vec!["--acp".to_owned()],
        env: BTreeMap::from([
            ("MODE".to_owned(), "agent".to_owned()),
            ("A".to_owned(), "a".to_owned()),
        ]),
    };
    let hook = BTreeMap::from([(OsString::from("MODE"), OsString::from("hook"))]);
    let inherited = [("MODE", "host"), ("HOME", "/home/x")]
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let spec = launch_spec(&agent, hook, inherited.into_iter(), std::env::temp_dir()).unwrap();
    let env: Vec<_> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .collect();
    assert_eq!(env, [("A", "a"), ("HOME", "/home/x"), ("MODE", "hook")]);
    assert_eq!(spec.args(), [OsString::from("--acp")]);
    assert!(
        launch_spec(
            &AgentLaunch {
                command: "agent".to_owned(),
                ..agent
            },
            BTreeMap::new(),
            std::iter::empty(),
            std::env::temp_dir()
        )
        .is_err()
    );
}

/// On Windows env names ignore case: an inherited `Path` must not stand beside (and beat) the
/// agent's `PATH`, nor an agent's `Mode` beside the hook's `MODE`.
#[test]
fn launch_spec_keeps_one_variable_per_name_ignoring_case() {
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::from([
            ("PATH".to_owned(), r"C:\node;C:\Windows".to_owned()),
            ("Mode".to_owned(), "agent".to_owned()),
        ]),
    };
    let hook = BTreeMap::from([(OsString::from("MODE"), OsString::from("hook"))]);
    let inherited = [
        ("Path", r"C:\Windows"),
        ("mode", "host"),
        ("HOME", "/home/x"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let spec = launch_spec(&agent, hook, inherited.into_iter(), std::env::temp_dir()).unwrap();
    let env: Vec<_> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .collect();
    assert_eq!(
        env,
        [
            ("HOME", "/home/x"),
            ("MODE", "hook"),
            ("PATH", r"C:\node;C:\Windows")
        ]
    );
}

/// Private issue #80: inherited secrets reach the redactor without touching the process env.
#[test]
fn inherited_secrets_are_redacted_from_the_stderr_tail() {
    let inherited = vec![
        (
            OsString::from("GITHUB_TOKEN"),
            OsString::from(FAKE_GITHUB_TOKEN),
        ),
        (
            OsString::from("GEMINI_API_KEY"),
            OsString::from(FAKE_GEMINI_KEY),
        ),
        (
            OsString::from("SMTP_SECRET"),
            OsString::from(FAKE_SMTP_SECRET),
        ),
        (
            OsString::from("DB_PASSWORD"),
            OsString::from(FAKE_DB_PASSWORD),
        ),
    ];
    let (mut pool, _link, _clock, mut rx) =
        run_agent(&fake_agent(scenario("stderr-secrets")), inherited);
    let ended = pool.run_until(ended(&mut rx));
    let Ended::Exited(DetachReason::Exited { code, stderr_tail }) = ended else {
        panic!("{ended:?}")
    };
    assert_eq!(code, Some(1));
    for secret in [
        FAKE_GITHUB_TOKEN,
        FAKE_GEMINI_KEY,
        FAKE_SMTP_SECRET,
        FAKE_DB_PASSWORD,
    ] {
        assert!(!stderr_tail.contains(secret), "{stderr_tail}");
    }
    assert!(stderr_tail.contains("[redacted]"), "{stderr_tail}");
    // The report waited for the pump's end: the agent's last line is in the tail.
    assert!(
        stderr_tail.contains("stderr-secrets: last line"),
        "{stderr_tail}"
    );
}

#[test]
fn stop_runs_the_ladder_and_reports_the_step() {
    let (mut pool, mut link, clock, mut rx) = run_agent(&fake_agent(scenario("idle")), Vec::new());
    link.stop();
    let ended = pool.run_until(ended(&mut rx));
    let Ended::Stopped {
        step,
        report: DetachReason::Exited { code, .. },
    } = ended
    else {
        panic!("{ended:?}")
    };
    // The idle agent exits as soon as its stdin closes, before the 2 s grace is up.
    assert_eq!((step, code), (StopStep::StdinClosed, Some(0)));
    link.stop();
    drop(clock);
}

#[test]
fn settle_waits_for_the_pipes_at_most_the_bound() {
    let mut pool = LocalPool::new();
    let clock = ManualClock::new(Timestamp::ZERO);
    let (tx, mut rx) = mpsc::unbounded();
    let task_clock = clock.clone();
    pool.spawner()
        .spawn_local(async move {
            tx.unbounded_send(settle(pending(), &task_clock).await)
                .unwrap();
        })
        .unwrap();
    pool.run_until_stalled();
    assert_eq!(clock.armed(), 1);
    clock.advance(
        PUMP_END_BOUND
            .checked_sub(std::time::Duration::from_millis(1))
            .unwrap(),
    );
    pool.run_until_stalled();
    assert!(rx.try_recv().is_err(), "settled before the bound");
    clock.advance(std::time::Duration::from_millis(1));
    pool.run_until_stalled();
    assert!(!rx.try_recv().unwrap());

    // Pipes that end first settle at once, with no time passing.
    assert!(pool.run_until(settle(ready(()), &clock)));
}
