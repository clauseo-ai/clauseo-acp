use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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

use super::{
    AgentInput, AgentLink, Ended, PUMP_END_BOUND, PUMP_END_ROUNDS, StopStep, keep_canonical,
    launch_spec, session_folder, settle, start,
};
use crate::clock::ManualClock;

#[cfg(target_os = "linux")]
mod held_pipe;

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
        BTreeMap::new(),
        &[],
        inherited.into_iter(),
        std::env::temp_dir(),
        None,
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
    let spec = launch_spec(
        &agent,
        hook,
        BTreeMap::new(),
        &[],
        inherited.into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
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
            BTreeMap::new(),
            &[],
            std::iter::empty(),
            std::env::temp_dir(),
            None,
        )
        .is_err()
    );
}

/// No inherited `GIT_*` reaches the agent, in any case of its name; one the agent's own settings
/// name is the lawyer's choice and stays. What the host forces wins over the settings and the
/// hook, whatever the case of their names.
#[test]
fn launch_spec_drops_inherited_git_and_forced_wins() {
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::from([
            ("GIT_AUTHOR_NAME".to_owned(), "Ada".to_owned()),
            (
                "initial_agent_mode".to_owned(),
                "agent-full-access".to_owned(),
            ),
        ]),
    };
    let hook = BTreeMap::from([(
        OsString::from("Initial_Agent_Mode"),
        OsString::from("agent"),
    )]);
    let forced = BTreeMap::from([(
        OsString::from("INITIAL_AGENT_MODE"),
        OsString::from("workspace-write"),
    )]);
    let inherited = [
        ("GIT_DIR", "/elsewhere/.git"),
        ("git_work_tree", "/elsewhere"),
        ("Git_Index_File", "/elsewhere/index"),
        ("GITHUB_TOKEN", FAKE_GITHUB_TOKEN),
        ("INITIAL_AGENT_MODE", "read-only"),
        ("HOME", "/home/x"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let spec = launch_spec(
        &agent,
        hook,
        forced,
        &[],
        inherited.into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    let env: Vec<_> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .collect();
    assert_eq!(
        env,
        [
            ("GITHUB_TOKEN", FAKE_GITHUB_TOKEN),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("HOME", "/home/x"),
            ("INITIAL_AGENT_MODE", "workspace-write"),
        ]
    );
}

/// Each dropped name is left out whatever its case and wherever it comes from: inherited, the
/// agent's variables or the hook's.
#[test]
fn launch_spec_leaves_out_dropped_names_from_every_source() {
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::from([
            ("codex_path".to_owned(), "/elsewhere/codex".to_owned()),
            ("KEEP".to_owned(), "1".to_owned()),
        ]),
    };
    let hook = BTreeMap::from([
        (OsString::from("Model_Provider"), OsString::from("other")),
        (OsString::from("HOOKED"), OsString::from("2")),
    ]);
    let inherited = [
        ("APP_SERVER_LOGS", "/tmp/logs"),
        ("default_auth_request", "api-key"),
        ("HOME", "/home/x"),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let spec = launch_spec(
        &agent,
        hook,
        BTreeMap::new(),
        &crate::agent::codex::DROPPED_VARS,
        inherited.into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    let names: Vec<_> = spec.env().keys().map(|k| k.to_str().unwrap()).collect();
    assert_eq!(names, ["HOME", "HOOKED", "KEEP"]);
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
    let spec = launch_spec(
        &agent,
        hook,
        BTreeMap::new(),
        &[],
        inherited.into_iter(),
        std::env::temp_dir(),
        None,
    )
    .unwrap();
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

/// A call the agent answered just before it exited posts its answer before `Ended`, however
/// late its task runs: here the task, holding its fence, waits after the answer until the owner
/// has seen the exit and the pipes end. Before the owner waited for fenced calls, `Ended` was
/// posted then, ahead of the answer, and a Codex prompt's limit error read as the agent's exit.
#[test]
fn a_call_answered_before_the_exit_posts_before_ended() {
    let (mut pool, link, clock, mut rx) =
        run_agent(&fake_agent(scenario("initialize-then-exit")), Vec::new());
    let answered = Rc::new(Cell::new(false));
    let (client, fence, calls) = (link.client.clone(), link.fence(), Rc::clone(&link.calls));
    let task_answered = Rc::clone(&answered);
    pool.spawner()
        .spawn_local(async move {
            let info = client.initialize().await;
            // Only once the owner is past the exit and the pipes, waiting for this call.
            futures::future::poll_fn(|cx| {
                if calls.owner_waits() {
                    std::task::Poll::Ready(())
                } else {
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
            fence.await;
            assert!(info.is_ok(), "{info:?}");
            task_answered.set(true);
        })
        .unwrap();
    let ended = pool.run_until(ended(&mut rx));
    assert!(answered.get(), "Ended came before the answer");
    assert!(matches!(ended, Ended::Exited(_)), "{ended:?}");
    drop((link, clock));
}

#[test]
fn settle_waits_for_the_pipes_at_most_the_bound() {
    let mut pool = LocalPool::new();
    let clock = ManualClock::new(Timestamp::ZERO);
    let (tx, mut rx) = mpsc::unbounded();
    let task_clock = clock.clone();
    pool.spawner()
        .spawn_local(async move {
            tx.unbounded_send(settle(pending(), || true, &task_clock).await)
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
    assert!(pool.run_until(settle(ready(()), || false, &clock)));
}

/// While what the agent wrote before it ended is still on its way, the bound passing does not
/// settle: each bound waits one more, up to the last.
#[test]
fn settle_waits_past_the_bound_while_the_output_is_on_its_way() {
    let mut pool = LocalPool::new();
    let clock = ManualClock::new(Timestamp::ZERO);
    let (tx, mut rx) = mpsc::unbounded();
    let posted = Rc::new(Cell::new(false));
    let (task_clock, task_posted) = (clock.clone(), Rc::clone(&posted));
    pool.spawner()
        .spawn_local(async move {
            tx.unbounded_send(settle(pending(), || task_posted.get(), &task_clock).await)
                .unwrap();
        })
        .unwrap();
    pool.run_until_stalled();
    for _ in 1..PUMP_END_ROUNDS {
        clock.advance(PUMP_END_BOUND);
        pool.run_until_stalled();
        assert!(rx.try_recv().is_err(), "settled with the output on its way");
        assert_eq!(clock.armed(), 1, "no bound armed again");
    }
    clock.advance(PUMP_END_BOUND);
    pool.run_until_stalled();
    assert!(!rx.try_recv().unwrap(), "past the last bound");

    // Once the output is posted, the next bound settles.
    let (tx, mut rx) = mpsc::unbounded();
    let (task_clock, task_posted) = (clock.clone(), Rc::clone(&posted));
    pool.spawner()
        .spawn_local(async move {
            tx.unbounded_send(settle(pending(), || task_posted.get(), &task_clock).await)
                .unwrap();
        })
        .unwrap();
    pool.run_until_stalled();
    clock.advance(PUMP_END_BOUND);
    pool.run_until_stalled();
    assert!(rx.try_recv().is_err(), "settled with the output on its way");
    posted.set(true);
    clock.advance(PUMP_END_BOUND);
    pool.run_until_stalled();
    assert!(!rx.try_recv().unwrap());
}

/// A folder reached through a symlink gives its real path; one that does not exist gives back
/// the stored spelling, untouched.
#[cfg(unix)]
#[test]
fn session_folder_is_the_real_path() {
    let root = std::env::temp_dir().join(format!(
        "clauseo-host-unit-session-folder-{}",
        std::process::id()
    ));
    crate::write::remove_dir_all(&root).unwrap();
    let real = root.join("real");
    crate::write::create_dir_all(&real).unwrap();
    let link = root.join("link");
    crate::write::symlink(&real, &link).unwrap();
    let resolved = dunce::canonicalize(&real).unwrap();
    assert_eq!(session_folder(&link), resolved);
    assert_eq!(session_folder(&link.join("..").join("real")), resolved);
    assert_eq!(session_folder(&resolved), resolved);
    let missing = root.join("gone").join("..").join("matter");
    assert_eq!(session_folder(&missing), missing);
    assert_eq!(session_folder(Path::new("")), Path::new(""));
    crate::write::remove_dir_all(&root).unwrap();
}

/// A canonical path that kept Windows' verbatim prefix (a UNC share, a mapped drive, or any
/// `\\?\` form `dunce` left) gives back the stored spelling; any other gives the canonical one.
/// Text only, so it runs on every OS.
#[test]
fn keep_canonical_refuses_a_verbatim_path() {
    let stored = Path::new(r"Z:\Matters\Lease");
    let kept = |canonical: &str| keep_canonical(Path::new(canonical), stored);
    assert_eq!(kept(r"\\?\UNC\srv\share\Matters\Lease"), stored);
    assert_eq!(kept(r"\\?\C:\Matters\Lease"), stored);
    assert_eq!(kept(r"C:\Matters\Lease"), Path::new(r"C:\Matters\Lease"));
    assert_eq!(kept(r"\\srv\share\Lease"), Path::new(r"\\srv\share\Lease"));
    assert_eq!(kept("/private/tmp/lease"), Path::new("/private/tmp/lease"));
}

/// Windows: a local folder's session spelling carries no `\\?\` prefix (the case `dunce` strips).
#[cfg(windows)]
#[test]
fn session_folder_of_a_local_folder_is_not_verbatim() {
    let folder = session_folder(&std::env::temp_dir());
    assert!(
        !folder.as_os_str().as_encoded_bytes().starts_with(br"\\?\"),
        "{folder:?}"
    );
    assert!(folder.is_absolute(), "{folder:?}");
}

/// R21: stetdoc's folder goes first on the merged `PATH`: the settings env's when it set one,
/// else the inherited one, matched ignoring case and kept under its own name.
#[test]
fn the_path_prepend_goes_first_on_the_merged_path() {
    use std::path::Path;
    let sep = if cfg!(windows) { ";" } else { ":" };
    let folder = Path::new(if cfg!(windows) {
        r"C:\stetdoc"
    } else {
        "/app/stetdoc"
    });
    let agent = |env: &[(&str, &str)]| AgentLaunch {
        command: std::env::temp_dir()
            .join("agent")
            .to_str()
            .unwrap()
            .to_owned(),
        args: Vec::new(),
        env: env
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    };
    let run = |launch: &AgentLaunch, inherited: Vec<(&str, &str)>| {
        let spec = launch_spec(
            launch,
            BTreeMap::new(),
            BTreeMap::new(),
            &[],
            inherited
                .into_iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
            std::env::temp_dir(),
            Some(folder),
        )
        .unwrap();
        spec.env()
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(name, value)| {
                (
                    name.to_str().unwrap().to_owned(),
                    value.to_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>()
    };
    let first = folder.to_str().unwrap();
    assert_eq!(
        run(&agent(&[]), vec![("Path", "/usr/bin")]),
        [("Path".to_owned(), format!("{first}{sep}/usr/bin"))]
    );
    assert_eq!(
        run(&agent(&[("PATH", "/opt/bin")]), vec![("Path", "/usr/bin")]),
        [("PATH".to_owned(), format!("{first}{sep}/opt/bin"))]
    );
    assert_eq!(
        run(&agent(&[]), vec![]),
        [("PATH".to_owned(), first.to_owned())]
    );
}
