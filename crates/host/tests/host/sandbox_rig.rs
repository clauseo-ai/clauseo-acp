//! The fake launcher seam the sandbox launch tests share (R17 PR-B2): what it was asked, what
//! it answers, and a host started with it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use futures::future::Either;

use clauseo_host::{Reservation, Sandbox, SandboxLaunch, SandboxLaunchers, SessionEnd, SessionKey};
use clauseo_process::{LaunchSpec, Launched, Launcher, PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{DomainEvent, ErrorCode, MatterId, StreamId, ThreadId, TurnId};
use clauseo_sandbox::{DenialSink, Detection, Policy, SetAside, Tier};

use crate::observe::Events;
use crate::support::{Dir, FAKE, Running, Scenario, wait};

/// The launch's wait (`codex::prepare::LAUNCH_WAIT`): the launch checks' and the seam's.
pub(crate) const LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// A launch's environment, as the spawn got it.
pub(crate) type Env = std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>;

/// A launcher that counts its launches, keeps each one's spec and starts each as the tests'
/// launcher does.
pub(crate) struct Counting {
    inner: PassthroughLauncher,
    launched: Arc<AtomicUsize>,
    specs: Mutex<Vec<LaunchSpec>>,
}

impl Counting {
    pub(crate) fn new(dir: &Dir) -> (Arc<Self>, Arc<AtomicUsize>) {
        let launched = Arc::new(AtomicUsize::new(0));
        let counting = Arc::new(Self {
            inner: dir.launcher(),
            launched: Arc::clone(&launched),
            specs: Mutex::new(Vec::new()),
        });
        (counting, launched)
    }

    /// The environments of its launches so far.
    pub(crate) fn envs(&self) -> Vec<Env> {
        self.specs().iter().map(|spec| spec.env().clone()).collect()
    }

    /// The specs of its launches so far.
    pub(crate) fn specs(&self) -> Vec<LaunchSpec> {
        self.specs.lock().unwrap().clone()
    }
}

impl Launcher for Counting {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, clauseo_process::Error> {
        self.launched.fetch_add(1, Ordering::SeqCst);
        self.specs.lock().unwrap().push(spec.clone());
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

/// One call of the seam's `launcher`, with what its `reserve` was given.
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub(crate) matter: MatterId,
    pub(crate) session: SessionKey,
    pub(crate) running: Vec<SessionKey>,
    pub(crate) policy: Policy,
}

/// The ports every fake reservation holds, in each jailed launch's `proxy_ports`.
pub(crate) const RESERVED_PORTS: [u16; 2] = [46_111, 46_112];

/// Counts a fake reservation's release: dropped with it.
struct Released(Arc<AtomicUsize>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A seam that records what it was asked and may wait for a word from the test first.
pub(crate) struct Fake {
    answer: Answer,
    launcher: Arc<Counting>,
    calls: Mutex<Vec<Call>>,
    reserved: Mutex<Vec<(SessionKey, Vec<SessionKey>)>>,
    released: Arc<AtomicUsize>,
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
            reserved: Mutex::new(Vec::new()),
            released: Arc::new(AtomicUsize::new(0)),
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

    /// The environments its launcher's launches got.
    pub(crate) fn envs(&self) -> Vec<Env> {
        self.launcher.envs()
    }

    /// The specs its launcher's launches got.
    pub(crate) fn specs(&self) -> Vec<LaunchSpec> {
        self.launcher.specs()
    }

    /// How many reservations were made.
    pub(crate) fn reserved(&self) -> usize {
        self.reserved.lock().unwrap().len()
    }

    /// How many reservations were released (dropped).
    pub(crate) fn released(&self) -> usize {
        self.released.load(Ordering::SeqCst)
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
    fn reserve(
        &self,
        matter: &MatterId,
        session: &SessionKey,
        running: &[SessionKey],
    ) -> Result<Reservation, clauseo_sandbox::Error> {
        self.reserved
            .lock()
            .unwrap()
            .push((*session, running.to_vec()));
        let released = Box::new(Released(Arc::clone(&self.released)));
        Ok(Reservation::fixed(RESERVED_PORTS.to_vec(), *matter, *session).with_guard(released))
    }

    fn launcher(
        &self,
        policy: Policy,
        matter: &MatterId,
        session: &SessionKey,
        reservation: Reservation,
        _opts: SpawnOptions,
        _denials: Arc<dyn DenialSink>,
    ) -> Result<SandboxLaunch, clauseo_sandbox::Error> {
        reservation.check(matter, session)?;
        let running = self
            .reserved
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(reserved, _)| reserved == session)
            .map(|(_, running)| running.clone())
            .unwrap_or_default();
        self.calls.lock().unwrap().push(Call {
            matter: *matter,
            session: *session,
            running,
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
    let (host, host_launched, _) = start_with(dir, fake, |config| config);
    (host, host_launched)
}

/// As [`start`], the config adjusted by `adjust` too, the sandbox's CA roots one fixed root;
/// also gives the host launcher, for its launches' environments.
pub(crate) fn start_with(
    dir: &Dir,
    fake: Option<Arc<Fake>>,
    adjust: impl FnOnce(clauseo_host::HostConfig) -> clauseo_host::HostConfig,
) -> (Running, Arc<AtomicUsize>, Arc<Counting>) {
    let (host_launcher, host_launched) = Counting::new(dir);
    let mut sandbox = Sandbox::new(Arc::new(tier_a), None)
        .with_ca_roots(Arc::new(|| vec![vec![0x30, 0x01, 0x00]]));
    if let Some(fake) = fake {
        let launchers: Arc<dyn SandboxLaunchers> = fake;
        sandbox = sandbox.with_launchers(launchers, SpawnOptions::new());
    }
    let launcher: Arc<dyn Launcher> = Arc::clone(&host_launcher) as _;
    let host = dir.start_launching(0, launcher, move |config| {
        adjust(config.with_sandbox(sandbox))
    });
    (host, host_launched, host_launcher)
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
