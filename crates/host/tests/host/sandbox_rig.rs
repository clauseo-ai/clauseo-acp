//! The fake launcher seam the sandbox launch tests share (R17 PR-B2): what it was asked, what
//! it answers, and a host started with it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use futures::future::Either;

use clauseo_host::{Sandbox, SandboxLaunch, SandboxLaunchers, SessionEnd, SessionKey};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{DomainEvent, ErrorCode, MatterId, StreamId, ThreadId, TurnId};
use clauseo_sandbox::{DenialSink, Detection, Policy, SetAside, Tier};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, wait};

/// The launch's wait (`codex::prepare::LAUNCH_WAIT`): the launch checks' and the seam's.
pub(crate) const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// A launcher that counts its launches and starts each as the tests' launcher does.
pub(crate) struct Counting {
    inner: PassthroughLauncher,
    launched: Arc<AtomicUsize>,
}

impl Counting {
    pub(crate) fn new(dir: &Dir) -> (Arc<Self>, Arc<AtomicUsize>) {
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

/// What the fake seam answers a launch.
pub(crate) enum Answer {
    /// A launcher, having set aside these names ([`moved`]).
    Launch(Vec<&'static str>),
    /// A refused policy whose message names the matter folder.
    Fail,
    /// The seam's thread panics.
    Panic,
}

/// One call of the seam's `launcher`.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub(crate) matter: MatterId,
    pub(crate) session: SessionKey,
    pub(crate) running: Vec<SessionKey>,
    pub(crate) policy: Policy,
}

/// A seam that records what it was asked and may wait for a word from the test first.
pub(crate) struct Fake {
    answer: Answer,
    launcher: Arc<Counting>,
    calls: Mutex<Vec<Call>>,
    after: Mutex<Vec<SessionEnd>>,
    after_moves: Mutex<Vec<&'static str>>,
    hold: Option<Mutex<mpsc::Receiver<()>>>,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    set_aside_root: PathBuf,
}

/// What [`Fake::held`] gives: the seam, its launches' count, the release and the asked signal.
pub(crate) type Held = (
    Arc<Fake>,
    Arc<AtomicUsize>,
    mpsc::Sender<()>,
    mpsc::Receiver<()>,
);

impl Fake {
    fn make(
        dir: &Dir,
        answer: Answer,
        hold: Option<mpsc::Receiver<()>>,
    ) -> (Self, Arc<AtomicUsize>) {
        let (launcher, launched) = Counting::new(dir);
        let fake = Self {
            answer,
            launcher,
            calls: Mutex::new(Vec::new()),
            after: Mutex::new(Vec::new()),
            after_moves: Mutex::new(Vec::new()),
            hold: hold.map(Mutex::new),
            entered: Mutex::new(None),
            set_aside_root: dir.data_dir().join("sandbox").join("set-aside"),
        };
        (fake, launched)
    }

    /// A seam answering `answer`, its launches counted in the returned count.
    pub(crate) fn new(dir: &Dir, answer: Answer) -> (Arc<Self>, Arc<AtomicUsize>) {
        let (fake, launched) = Self::make(dir, answer, None);
        (Arc::new(fake), launched)
    }

    /// As [`Fake::new`], answering only once the returned sender sends or drops; the receiver
    /// hears when it was asked.
    pub(crate) fn held(dir: &Dir, answer: Answer) -> Held {
        let (release, hold) = mpsc::channel();
        let (entered, asked) = mpsc::channel();
        let (fake, launched) = Self::make(dir, answer, Some(hold));
        *fake.entered.lock().unwrap() = Some(entered);
        (Arc::new(fake), launched, release, asked)
    }

    /// Every `after_session` from now on moves these names ([`moved`]).
    pub(crate) fn move_after(&self, names: &[&'static str]) {
        *self.after_moves.lock().unwrap() = names.to_vec();
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn after(&self) -> Vec<SessionEnd> {
        self.after.lock().unwrap().clone()
    }

    pub(crate) fn after_matters(&self) -> Vec<MatterId> {
        self.after().iter().map(|end| end.matter).collect()
    }

    /// Waits until `after_session` was called `n` times.
    pub(crate) fn until_after(&self, n: usize) {
        wait("the after-session calls", async {
            while self.after.lock().unwrap().len() < n {
                async_io::Timer::after(Duration::from_millis(5)).await;
            }
        });
    }

    /// The set-aside list for `names` under `matter`.
    fn set_aside(&self, matter: &Path, names: &[&'static str]) -> Vec<SetAside> {
        names
            .iter()
            .map(|name| SetAside {
                from: moved(matter, name),
                to: self
                    .set_aside_root
                    .join(name.trim_start_matches(['/', '\\', '?'])),
            })
            .collect()
    }
}

/// Where a test's name was: `/…` is that absolute path (outside the matter), `verbatim:<name>`
/// is `<name>` in the matter behind Windows' `\\?\` prefix, any other is in the matter.
pub(crate) fn moved(matter: &Path, name: &str) -> PathBuf {
    if name.starts_with('/') {
        return PathBuf::from(name);
    }
    match name.strip_prefix("verbatim:") {
        Some(rest) => PathBuf::from(format!(r"\\?\{}", matter.join(rest).display())),
        None => matter.join(name),
    }
}

impl SandboxLaunchers for Fake {
    fn launcher(
        &self,
        policy: Policy,
        matter: &MatterId,
        session: &SessionKey,
        running: &[SessionKey],
        _opts: SpawnOptions,
        _denials: Arc<dyn DenialSink>,
    ) -> Result<SandboxLaunch, clauseo_sandbox::Error> {
        self.calls.lock().unwrap().push(Call {
            matter: *matter,
            session: *session,
            running: running.to_vec(),
            policy: policy.clone(),
        });
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _told = entered.send(());
        }
        if let Some(hold) = &self.hold {
            let _released = hold.lock().unwrap().recv();
        }
        match &self.answer {
            Answer::Launch(names) => {
                let set_aside = self.set_aside(policy.matter(), names);
                let launcher: Arc<dyn Launcher> = Arc::clone(&self.launcher) as _;
                Ok(SandboxLaunch::new(launcher, set_aside))
            }
            Answer::Fail => Err(clauseo_sandbox::Error::Policy(format!(
                "the matter {} holds the home folder",
                policy.matter().display()
            ))),
            Answer::Panic => panic!("the fake seam panics"),
        }
    }

    fn after_session(&self, end: SessionEnd) -> Vec<SetAside> {
        let names = self.after_moves.lock().unwrap().clone();
        let moved = self.set_aside(end.policy.matter(), &names);
        self.after.lock().unwrap().push(end);
        moved
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
pub(crate) fn dir(test: &str, scenario: &'static str) -> Dir {
    Dir::new(test, &[(FAKE, Scenario::Bundled(scenario))])
}

/// Starts a host whose own launcher is counted (the returned count), with `fake` as the seam's
/// launchers when given, else the seam alone.
pub(crate) fn start(dir: &Dir, fake: Option<Arc<Fake>>) -> (Running, Arc<AtomicUsize>) {
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
pub(crate) fn failed(events: &Events, turn: TurnId) -> Option<(ErrorCode, Option<String>)> {
    events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == turn => {
            Some((failed.code, failed.detail.clone()))
        }
        _ => None,
    })
}

pub(crate) fn attached(events: &Events) -> usize {
    events.count(|event| matches!(event, DomainEvent::AgentSessionAttached(_)))
}

/// The texts of the thread's notices so far.
pub(crate) fn notices(events: &Events) -> Vec<String> {
    events
        .events()
        .filter_map(|event| match event {
            DomainEvent::Notice(notice) => Some(notice.text.clone()),
            _ => None,
        })
        .collect()
}

/// Waits until `thread`'s log holds `n` notices, and gives their texts.
pub(crate) fn until_notices(host: &Running, thread: ThreadId, n: usize) -> Vec<String> {
    let mut events = host.events(StreamId::Thread(thread));
    events.until("the notices", |events| notices(events).len() >= n);
    notices(&events)
}

/// The entries of the data folder's `run` folder.
pub(crate) fn run_entries(dir: &Dir) -> Vec<PathBuf> {
    std::fs::read_dir(dir.data_dir().join("run"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

/// Waits until the data folder's `run` folder is empty.
pub(crate) fn until_no_run_folder(dir: &Dir) {
    wait("no run folder", async {
        while !run_entries(dir).is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// `path` resolved, as the policy holds it.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap()
}

/// Shuts `host` down, running `meanwhile` once the shutdown was sent; both must end.
pub(crate) fn shutdown_while(mut host: Running, meanwhile: impl Future<Output = ()>) {
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
