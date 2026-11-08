//! Recording fakes of the sandbox seam (decision record 0051, R17 PR-B1): a set-up with a
//! scripted `state()` sequence and scripted `run()`, `remove()` and `forget_matter()` outcomes,
//! one of which can block until the test releases it, and a detection with a scripted sequence.
//! Each sequence repeats its last value once the earlier ones are used.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll};

use clauseo_host::{AppSubscription, HostHandle, Sandbox};
use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, HostCommand, HostError, HostEvent, Principal, ProtectionView,
    QueryResult,
};
use clauseo_sandbox::{Detection, NoSandbox, SandboxSetup, SetupError, SetupState, Tier};
use futures::StreamExt as _;

use crate::support::{Running, command_id, wait, within};

/// A scripted sequence: each call takes the next value, and the last repeats.
pub(crate) struct Script<T: Clone>(Mutex<VecDeque<T>>);

impl<T: Clone> Script<T> {
    pub(crate) fn new(values: Vec<T>) -> Self {
        assert!(!values.is_empty(), "a script needs a value");
        Self(Mutex::new(values.into()))
    }

    fn next(&self) -> T {
        let mut values = self.0.lock().unwrap();
        if values.len() > 1 {
            values.pop_front().unwrap()
        } else {
            values.front().cloned().unwrap()
        }
    }
}

/// A fake set-up.
pub(crate) struct FakeSetup {
    states: Script<SetupState>,
    runs: Script<Result<(), SetupError>>,
    removes: Script<Result<(), SetupError>>,
    forgets: Script<Result<(), SetupError>>,
    /// Which call waits at the gate, and its ends.
    gated: Gated,
    gate: Option<Arc<GateEnds>>,
    /// `run()` panics.
    panics: bool,
    pub(crate) run_calls: AtomicUsize,
    pub(crate) remove_calls: AtomicUsize,
    /// The matters `forget_matter()` was called for, and the names of the threads it ran on.
    pub(crate) forgotten: Mutex<Vec<(clauseo_protocol::MatterId, String)>>,
    /// The names of the threads `state()`, `run()` and `remove()` were called on.
    pub(crate) threads: Mutex<Vec<String>>,
}

/// The fake's ends of a gate: where the gated call says it started, and where it waits.
type GateEnds = (Mutex<mpsc::Sender<()>>, Mutex<mpsc::Receiver<()>>);

/// Which call says it started, then waits until the test releases it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gated {
    /// None.
    No,
    /// `run()`.
    Run,
    /// `remove()`.
    Remove,
    /// `forget_matter()`, once it has noted its call.
    Forget,
    /// Every detection after the first (start-up's).
    Detect,
}

/// Waits at `gate`: says it started, then waits for a release (a dropped sender, at the test's
/// end, releases it too).
fn pass(gate: &GateEnds) {
    gate.0.lock().unwrap().send(()).unwrap();
    let _released = gate.1.lock().unwrap().recv();
}

/// A gated fake's handles: `run()`'s starts, and the sender that releases it.
pub(crate) struct Gate {
    pub(crate) started: mpsc::Receiver<()>,
    pub(crate) release: mpsc::Sender<()>,
}

impl FakeSetup {
    fn note_thread(&self) {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        self.threads.lock().unwrap().push(name);
    }
}

impl SandboxSetup for FakeSetup {
    fn state(&self) -> SetupState {
        self.note_thread();
        self.states.next()
    }

    fn run(&self) -> Result<(), SetupError> {
        self.note_thread();
        self.run_calls.fetch_add(1, Ordering::SeqCst);
        if self.gated == Gated::Run
            && let Some(gate) = &self.gate
        {
            pass(gate);
        }
        assert!(!self.panics, "the fake set-up panics, as the test asked");
        self.runs.next()
    }

    fn forget_matter(&self, matter: &clauseo_protocol::MatterId) -> Result<(), SetupError> {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        self.forgotten.lock().unwrap().push((*matter, name));
        if self.gated == Gated::Forget
            && let Some(gate) = &self.gate
        {
            pass(gate);
        }
        self.forgets.next()
    }

    fn remove(&self) -> Result<(), SetupError> {
        self.note_thread();
        self.remove_calls.fetch_add(1, Ordering::SeqCst);
        if self.gated == Gated::Remove
            && let Some(gate) = &self.gate
        {
            pass(gate);
        }
        self.removes.next()
    }
}

/// What a rig is built from.
pub(crate) struct Plan {
    pub(crate) detections: Vec<Detection>,
    pub(crate) states: Vec<SetupState>,
    pub(crate) runs: Vec<Result<(), SetupError>>,
    pub(crate) removes: Vec<Result<(), SetupError>>,
    pub(crate) forgets: Vec<Result<(), SetupError>>,
    pub(crate) gated: Gated,
    pub(crate) panics: bool,
}

impl Plan {
    /// Detected as `detections`, the set-up in `states`; every run, removal and forget succeeds,
    /// at once.
    pub(crate) fn new(detections: Vec<Detection>, states: Vec<SetupState>) -> Self {
        Self {
            detections,
            states,
            runs: vec![Ok(())],
            removes: vec![Ok(())],
            forgets: vec![Ok(())],
            gated: Gated::No,
            panics: false,
        }
    }
}

/// A seam from `plan`: the seam, its set-up, its gate when gated, and its detection calls.
pub(crate) struct Rig {
    pub(crate) sandbox: Sandbox,
    pub(crate) setup: Arc<FakeSetup>,
    pub(crate) gate: Option<Gate>,
    pub(crate) detections: Arc<AtomicUsize>,
}

pub(crate) fn rig(plan: Plan) -> Rig {
    let (gate, handles) = if plan.gated == Gated::No {
        (None, None)
    } else {
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        (
            Some(Arc::new((Mutex::new(started_tx), Mutex::new(release_rx)))),
            Some(Gate { started, release }),
        )
    };
    let setup = Arc::new(FakeSetup {
        states: Script::new(plan.states),
        runs: Script::new(plan.runs),
        removes: Script::new(plan.removes),
        forgets: Script::new(plan.forgets),
        gated: plan.gated,
        gate: gate.clone(),
        panics: plan.panics,
        run_calls: AtomicUsize::new(0),
        remove_calls: AtomicUsize::new(0),
        forgotten: Mutex::new(Vec::new()),
        threads: Mutex::new(Vec::new()),
    });
    let script = Arc::new(Script::new(plan.detections));
    let detections = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&detections);
    let detect_gate = (plan.gated == Gated::Detect).then_some(gate).flatten();
    let detect = Arc::new(move || {
        let calls = counted.fetch_add(1, Ordering::SeqCst);
        if calls > 0
            && let Some(gate) = &detect_gate
        {
            pass(gate);
        }
        script.next()
    });
    let shared: Arc<dyn SandboxSetup> = Arc::<FakeSetup>::clone(&setup);
    Rig {
        sandbox: Sandbox::new(detect, Some(shared)),
        setup,
        gate: handles,
        detections,
    }
}

/// Tier C for `reason`.
pub(crate) fn c(reason: NoSandbox) -> Detection {
    Detection {
        tier: Tier::C,
        reason: Some(reason),
    }
}

/// Tier B, set up.
pub(crate) fn b() -> Detection {
    Detection {
        tier: Tier::B,
        reason: None,
    }
}

/// The host's protection view.
pub(crate) fn view(host: &Running) -> ProtectionView {
    match host.query(clauseo_protocol::HostQuery::Protection).unwrap() {
        QueryResult::Protection(view) => view,
        other => panic!("not a protection view: {other:?}"),
    }
}

/// A command's answer, sent to the host now and awaited later.
pub(crate) type Sent = Pin<Box<dyn Future<Output = Result<CommandOutcome, HostError>> + Send>>;

/// Sends `command` to the host under a fresh id: the request is in the host's channel when this
/// returns (the call's first poll sends it), so what is sent after it comes after it.
pub(crate) fn send(handle: &HostHandle, command: HostCommand) -> Sent {
    let handle = handle.clone();
    let envelope = CommandEnvelope {
        command_id: command_id(),
        command,
    };
    let mut sent: Sent = Box::pin(async move { handle.dispatch(envelope).await });
    let mut cx = Context::from_waker(futures::task::noop_waker_ref());
    if let Poll::Ready(answer) = sent.as_mut().poll(&mut cx) {
        return Box::pin(async move { answer });
    }
    sent
}

/// An app subscription.
pub(crate) fn subscribe(host: &Running) -> AppSubscription {
    wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap()
}

/// The protection events on `sub` (`ProtectionViewChanged` and `ProtectionChanged`), skipping
/// the others, until `count` have come.
pub(crate) fn protection_events(sub: &mut AppSubscription, count: usize) -> Vec<HostEvent> {
    wait("the protection events", async {
        let mut found = Vec::new();
        while found.len() < count {
            let item = sub.next().await.expect("the app stream ended");
            let event = crate::settings::decode(&item);
            if is_protection(&event) {
                found.push(event);
            }
        }
        found
    })
}

/// Every protection event left on `sub` until its stream ends (the host stopped).
pub(crate) fn protection_events_to_end(sub: &mut AppSubscription) -> Vec<HostEvent> {
    within(async {
        let mut found = Vec::new();
        while let Some(item) = sub.next().await {
            let event = crate::settings::decode(&item);
            if is_protection(&event) {
                found.push(event);
            }
        }
        found
    })
    .expect("the app stream did not end")
}

fn is_protection(event: &HostEvent) -> bool {
    matches!(
        event,
        HostEvent::ProtectionViewChanged { .. } | HostEvent::ProtectionChanged { .. }
    )
}
