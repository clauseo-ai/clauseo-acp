//! The launcher seam (decision record 0051, R17 PR-B2): with `Sandbox::with_launchers` every
//! agent launch asks the seam for its launcher off the actor, with the launch's policy, and
//! spawns through it; the names it set aside give one notice; its `after_session` follows the
//! agent's exit. Without launchers the launch is the host launcher's, as before.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use futures::future::Either;

use clauseo_host::{Sandbox, SandboxLaunch, SandboxLaunchers};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{DomainEvent, ErrorCode, MatterId, StopReason, StreamId, TurnId};
use clauseo_sandbox::{DenialSink, Detection, Policy, SetAside, Tier};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, matter_with_thread, wait};

/// The launch's wait (`codex::prepare::LAUNCH_WAIT`): the launch checks' and the seam's.
const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// A launcher that counts its launches and starts each as the tests' launcher does.
struct Counting {
    inner: PassthroughLauncher,
    launched: Arc<AtomicUsize>,
}

impl Counting {
    fn new(dir: &Dir) -> (Arc<Self>, Arc<AtomicUsize>) {
        let launched = Arc::new(AtomicUsize::new(0));
        let counting = Arc::new(Self {
            inner: dir.launcher(),
            launched: Arc::clone(&launched),
        });
        (counting, launched)
    }
}

impl Launcher for Counting {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.launched.fetch_add(1, Ordering::SeqCst);
        self.inner.launch(spec)
    }
}

/// What the fake seam answers.
enum Answer {
    /// A launcher, having set aside these names (relative to the matter).
    Launch(Vec<&'static str>),
    /// A refused policy whose message names the matter folder.
    Fail,
}

/// A seam that records what it was asked and may wait for a word from the test first.
struct Fake {
    answer: Answer,
    launcher: Arc<Counting>,
    calls: Mutex<Vec<(MatterId, Policy)>>,
    after: Mutex<Vec<MatterId>>,
    hold: Option<Mutex<mpsc::Receiver<()>>>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    set_aside_root: PathBuf,
}

impl Fake {
    /// A seam answering `answer`, its launches counted in the returned count.
    fn new(dir: &Dir, answer: Answer) -> (Arc<Self>, Arc<AtomicUsize>) {
        let (launcher, launched) = Counting::new(dir);
        let fake = Arc::new(Self {
            answer,
            launcher,
            calls: Mutex::new(Vec::new()),
            after: Mutex::new(Vec::new()),
            hold: None,
            entered: Mutex::new(None),
            set_aside_root: dir.data_dir().join("sandbox").join("set-aside"),
        });
        (fake, launched)
    }

    /// As [`Fake::new`], answering only once the returned sender sends or drops; the receiver
    /// hears when it was asked.
    fn held(
        dir: &Dir,
        answer: Answer,
    ) -> (
        Arc<Self>,
        Arc<AtomicUsize>,
        mpsc::Sender<()>,
        mpsc::Receiver<()>,
    ) {
        let (launcher, launched) = Counting::new(dir);
        let (release, hold) = mpsc::channel();
        let (entered, asked) = mpsc::channel();
        let fake = Arc::new(Self {
            answer,
            launcher,
            calls: Mutex::new(Vec::new()),
            after: Mutex::new(Vec::new()),
            hold: Some(Mutex::new(hold)),
            entered: Mutex::new(Some(entered)),
            set_aside_root: dir.data_dir().join("sandbox").join("set-aside"),
        });
        (fake, launched, release, asked)
    }

    fn calls(&self) -> Vec<(MatterId, Policy)> {
        self.calls.lock().unwrap().clone()
    }

    fn after(&self) -> Vec<MatterId> {
        self.after.lock().unwrap().clone()
    }
}

impl SandboxLaunchers for Fake {
    fn launcher(
        &self,
        policy: Policy,
        matter: &MatterId,
        _opts: SpawnOptions,
        _denials: Arc<dyn DenialSink>,
    ) -> Result<SandboxLaunch, clauseo_sandbox::Error> {
        self.calls.lock().unwrap().push((*matter, policy.clone()));
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _told = entered.send(());
        }
        if let Some(hold) = &self.hold {
            let _released = hold.lock().unwrap().recv();
        }
        match &self.answer {
            Answer::Launch(names) => {
                let set_aside = names
                    .iter()
                    .map(|name| SetAside {
                        from: policy.matter().join(name),
                        to: self.set_aside_root.join(name),
                    })
                    .collect();
                let launcher: Arc<dyn Launcher> = Arc::clone(&self.launcher) as _;
                Ok(SandboxLaunch::new(launcher, set_aside))
            }
            Answer::Fail => Err(clauseo_sandbox::Error::Policy(format!(
                "the matter {} holds the home folder",
                policy.matter().display()
            ))),
        }
    }

    fn after_session(&self, matter: &MatterId) {
        self.after.lock().unwrap().push(*matter);
    }
}

/// Tier A, as macOS' detection says.
fn tier_a() -> Detection {
    Detection {
        tier: Tier::A,
        reason: None,
    }
}

/// A dir whose fake agent plays `scenario`.
fn dir(test: &str, scenario: &'static str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Bundled(scenario))])
}

/// Starts a host whose own launcher is counted (the returned count), with `fake` as the seam's
/// launchers when given, else the seam alone.
fn start(dir: &Dir, fake: Option<Arc<Fake>>) -> (Running, Arc<AtomicUsize>) {
    let (host_launcher, host_launched) = Counting::new(dir);
    let mut sandbox = Sandbox::new(Arc::new(tier_a), None);
    if let Some(fake) = fake {
        let launchers: Arc<dyn SandboxLaunchers> = fake;
        sandbox = sandbox.with_launchers(launchers, SpawnOptions::new());
    }
    let host = dir.start_launching(0, host_launcher, move |config| config.with_sandbox(sandbox));
    (host, host_launched)
}

/// `turn`'s `TurnFailed`, if logged.
fn failed(events: &Events, turn: TurnId) -> Option<(ErrorCode, Option<String>)> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => {
            Some((failed.code, failed.detail.clone()))
        }
        _ => None,
    })
}

fn attached(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
}

/// The entries of the data folder's `run` folder.
fn run_entries(dir: &Dir) -> Vec<PathBuf> {
    std::fs::read_dir(dir.data_dir().join("run"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

/// Waits until the data folder's `run` folder is empty.
fn until_no_run_folder(dir: &Dir) {
    wait("no run folder", async {
        while !run_entries(dir).is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// `path` resolved, as the policy holds it.
fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

#[test]
fn with_launchers_a_launch_asks_once_with_its_policy_and_spawns_through_the_seam() {
    let dir = dir("sandbox_launch_seam", "prompt-stream");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );

    let calls = fake.calls();
    assert_eq!(calls.len(), 1, "one launcher per launch");
    let (asked, policy) = &calls[0];
    assert_eq!(*asked, matter);
    assert_eq!(policy.matter(), canonical(&folder));
    // TMPDIR is the session's `run/<session>/tmp`; the run folder itself is not granted.
    let tmp = policy.session_tmpdir().expect("the session's tmp");
    assert_eq!(tmp.file_name().unwrap(), "tmp");
    let session = tmp.parent().unwrap();
    assert_eq!(
        session.parent().unwrap(),
        canonical(&dir.data_dir().join("run"))
    );
    assert_ne!(policy.state(), session);
    assert!(policy.state().starts_with(session), "{policy:?}");
    // The agent's install folder, read and run.
    let install = canonical(clauseo_fake_agent::bin_path().unwrap().parent().unwrap());
    assert_eq!(policy.runtime(), [install]);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 1);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    // No set-aside, no notice.
    assert_eq!(
        events.count(|event| matches!(event, DomainEvent::Notice(_))),
        0
    );
    host.shutdown();
    assert_eq!(fake.calls().len(), 1);
    // The agent stopped with the host: its after-session ran once before shutdown answered.
    assert_eq!(fake.after(), [matter]);
}

#[test]
fn without_launchers_nothing_is_asked_and_the_host_launcher_spawns() {
    let dir = dir("sandbox_launch_none", "prompt-stream");
    let (host, host_launched) = start(&dir, None);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    assert_eq!(host_launched.load(Ordering::SeqCst), 1);
    host.shutdown();
}

#[test]
fn a_launcher_error_fails_the_launch_naming_its_cause_and_leaves_nothing() {
    let dir = dir("sandbox_launch_error", "prompt-stream");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Fail);
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    let (code, detail) = failed(&events, turn).expect("TurnFailed");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    let detail = detail.unwrap_or_default();
    assert!(detail.contains("sandbox"), "{detail}");
    let matter_path = canonical(&folder);
    for path in [matter_path.as_path(), folder.as_path(), dir.data_dir()] {
        assert!(!detail.contains(&*path.to_string_lossy()), "{detail}");
    }
    host.settle();
    events.drain_ready();
    assert_eq!(attached(&events), 0, "{}", events.dump());
    assert!(run_entries(&dir).is_empty(), "{:?}", run_entries(&dir));
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    host.shutdown();
    assert!(
        fake.after().is_empty(),
        "no agent spawned, so no after-session"
    );
}

#[test]
fn a_launcher_held_past_the_bound_fails_the_launch_while_the_host_answers() {
    let dir = dir("sandbox_launch_held", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    // The launch checks' wait, then the seam's.
    host.wait_armed(LAUNCH_WAIT);
    host.wait_armed(LAUNCH_WAIT);
    // The actor answers while the seam holds.
    assert_eq!(host.threads(matter).len(), 1);
    host.clock.advance(LAUNCH_WAIT);
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    let (code, detail) = failed(&events, turn).expect("TurnFailed");
    assert_eq!(code, ErrorCode::AgentLaunchFailed);
    assert!(detail.unwrap_or_default().contains("sandbox"));
    // The late answer is taken back on its thread: nothing spawns, its run folder goes.
    drop(release);
    until_no_run_folder(&dir);
    host.settle();
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    host.shutdown();
    assert_eq!(fake.after(), [] as [MatterId; 0]);
}

#[test]
fn two_names_set_aside_give_one_notice_naming_both_relative() {
    let dir = dir("sandbox_launch_set_aside", "prompt-stream");
    let (fake, _) = Fake::new(&dir, Answer::Launch(vec![".git", "sub/AGENTS.md"]));
    let (host, _) = start(&dir, Some(fake));
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::EndTurn,
        "{}",
        events.dump()
    );
    let notices: Vec<String> = events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    let text = &notices[0];
    assert!(
        text.contains(".git") && text.contains("sub/AGENTS.md"),
        "{text}"
    );
    for path in [
        canonical(&folder),
        folder.clone(),
        dir.data_dir().to_owned(),
    ] {
        assert!(!text.contains(&*path.to_string_lossy()), "{text}");
    }
    host.shutdown();
}

#[test]
fn after_session_is_called_once_when_the_agent_exits() {
    let dir = dir("sandbox_launch_after", "exit-mid-turn");
    let (fake, seam_launched) = Fake::new(&dir, Answer::Launch(Vec::new()));
    let (host, _) = start(&dir, Some(Arc::clone(&fake)));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Start the answer.");
    assert_eq!(
        events.until_turn_ended(turn).stop,
        StopReason::Failed,
        "{}",
        events.dump()
    );
    assert_eq!(
        failed(&events, turn).map(|(code, _)| code),
        Some(ErrorCode::AgentExited)
    );
    wait("the after-session call", async {
        while fake.after().is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    until_no_run_folder(&dir);
    host.settle();
    host.shutdown();
    assert_eq!(fake.after(), [matter]);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 1);
}

/// Shuts `host` down, running `meanwhile` once the shutdown was sent; both must end.
fn shutdown_while(mut host: Running, meanwhile: impl Future<Output = ()>) {
    let result = wait("shutdown", async {
        let shutdown = std::pin::pin!(host.handle.shutdown());
        let meanwhile = std::pin::pin!(meanwhile);
        match futures::future::select(shutdown, meanwhile).await {
            Either::Left((result, meanwhile)) => {
                meanwhile.await;
                result
            }
            Either::Right(((), shutdown)) => shutdown.await,
        }
    });
    result.unwrap_or_else(|e| panic!("shutdown: {e}"));
    if let Some(join) = host.take_join_if_any() {
        join.join().unwrap_or_else(|e| panic!("join: {e}"));
    }
}

/// A held seam keeps shutdown waiting no longer than a launch's checks would (the launch's
/// wait); its answer during shutdown is taken back, so nothing spawns and the run folder goes.
#[test]
fn shutdown_during_a_held_launcher_ends_cleanly_and_the_late_answer_spawns_nothing() {
    let dir = dir("sandbox_launch_shutdown", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(vec![".git"]));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[(
            "brief.txt",
            "x
",
        )],
        FAKE,
    );
    host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    shutdown_while(host, async move {
        // The shutdown is under way by then; the seam answers into it.
        async_io::Timer::after(Duration::from_millis(50)).await;
        drop(release);
    });
    until_no_run_folder(&dir);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(fake.after(), [] as [MatterId; 0]);
}

/// A seam that never answers holds shutdown only until the launch's wait passes on the host
/// clock; the answer that comes after the host is gone spawns nothing.
#[test]
fn shutdown_past_a_seam_that_never_answers_ends_at_the_launch_wait() {
    let dir = dir("sandbox_launch_shutdown_wait", "prompt-stream");
    let (fake, seam_launched, release, asked) = Fake::held(&dir, Answer::Launch(Vec::new()));
    let (host, host_launched) = start(&dir, Some(Arc::clone(&fake)));
    let (_, thread, _) = matter_with_thread(
        &dir,
        &host,
        "m",
        &[(
            "brief.txt",
            "x
",
        )],
        FAKE,
    );
    host.prompt(thread, "Look at the matter.");
    asked.recv().unwrap();
    host.wait_armed(LAUNCH_WAIT);
    host.wait_armed(LAUNCH_WAIT);
    let clock = host.clock.clone();
    shutdown_while(host, async move {
        async_io::Timer::after(Duration::from_millis(50)).await;
        clock.advance(LAUNCH_WAIT);
    });
    drop(release);
    until_no_run_folder(&dir);
    assert_eq!(seam_launched.load(Ordering::SeqCst), 0);
    assert_eq!(host_launched.load(Ordering::SeqCst), 0);
    assert_eq!(fake.after(), [] as [MatterId; 0]);
}
