//! The sign-in hook (decision record 0048, ruling 2; P4b): a [`LoginCheck`] set on the config
//! is asked off the actor at each Codex launch, with a bounded wait. While desktop says the
//! login is reused, an answer but `Shared` refuses the launch, holds the turn for the sign-in
//! and sends a [`LoginSplit`]; while the file is shared, a typed `/login` is refused with
//! `signin.login_shared`. A check that does not answer is no sign-in answer, an unset hook
//! checks nothing, no log of it names a path, and `remove_login_links` never waits on the actor.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use clauseo_host::{LoginCheck, LoginLink, LoginSplit};
use clauseo_protocol::{AgentId, DomainEvent, ErrorCode, StopReason, StreamId};

use crate::codex::{CODEX, Recording, codex_dir, run_turn};
use crate::queue::held;
use crate::support::{Dir, matter_with_thread, send_prompt, wait};

/// A hook that answers what the test set, records what it was asked, and may wait for a word
/// from the test first.
struct Fake {
    answer: Mutex<LoginLink>,
    asked: Mutex<Vec<(AgentId, PathBuf)>>,
    hold: Option<Mutex<mpsc::Receiver<()>>>,
}

impl Fake {
    fn new(answer: LoginLink) -> Arc<Self> {
        Arc::new(Self {
            answer: Mutex::new(answer),
            asked: Mutex::new(Vec::new()),
            hold: None,
        })
    }

    /// A hook that answers only once the sender sends or drops.
    fn held(answer: LoginLink) -> (Arc<Self>, mpsc::Sender<()>) {
        let (release, hold) = mpsc::channel();
        let fake = Arc::new(Self {
            answer: Mutex::new(answer),
            asked: Mutex::new(Vec::new()),
            hold: Some(Mutex::new(hold)),
        });
        (fake, release)
    }

    fn set(&self, answer: LoginLink) {
        *self.answer.lock().unwrap() = answer;
    }

    fn asked(&self) -> Vec<(AgentId, PathBuf)> {
        self.asked.lock().unwrap().clone()
    }
}

impl LoginCheck for Fake {
    fn check(&self, agent: &AgentId, agent_home: &Path) -> LoginLink {
        self.asked
            .lock()
            .unwrap()
            .push((agent.clone(), agent_home.to_owned()));
        if let Some(hold) = &self.hold {
            let _released = hold.lock().unwrap().recv();
        }
        *self.answer.lock().unwrap()
    }
}

fn codex() -> AgentId {
    AgentId::new(CODEX).unwrap()
}

/// Clauseo's Codex home under the test's data folder.
fn home(dir: &Dir) -> PathBuf {
    dir.data_dir().join("agents").join(CODEX).join("home")
}

#[test]
fn the_hook_refuses_a_typed_login_with_login_shared_while_the_file_is_shared() {
    let dir = codex_dir("login_check_shared", "codex-handshake");
    let fake = Fake::new(LoginLink::Shared);
    let hook = Arc::clone(&fake);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));

    // Reused: the link exists, so the guard refuses before any launch.
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    let error = host.refused(send_prompt(thread, "/login"), ErrorCode::SigninLoginShared);
    assert!(
        !error.message().contains(&*dir.data_dir().to_string_lossy()),
        "{error}"
    );
    // Not reused, but the launch's check finds the file shared: still refused.
    wait("not reused", host.handle.set_reused_login(codex(), false)).unwrap();
    run_turn(&host, &mut events, thread);
    host.refused(
        send_prompt(thread, " /LOGIN now"),
        ErrorCode::SigninLoginShared,
    );
    assert_eq!(fake.asked(), [(codex(), home(&dir))]);
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn an_unset_hook_checks_nothing() {
    let dir = codex_dir("login_check_unset", "codex-handshake");
    let host = dir.start(0);
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    // No split check: the launch goes on; no guard: `/login` is the agent's one prompt.
    let (turn, _) = host.prompt(thread, "/login");
    assert_eq!(events.until_turn_ended(turn).stop, StopReason::EndTurn);
    host.settle();
    events.drain_ready();
    events.assert_no_mismatch();
    assert!(host.handle.login_splits().try_recv().is_err());
    host.shutdown();
}

#[test]
fn a_split_refuses_the_launch_holds_the_turn_and_sends_the_split() {
    let dir = codex_dir("login_check_split", "codex-handshake");
    let fake = Fake::new(LoginLink::Split);
    let hook = Arc::clone(&fake);
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let host = dir.start_launching(0, Arc::new(recording), |config| {
        config.with_login_check(hook)
    });
    let splits = host.handle.login_splits();
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();

    let (first, _) = host.prompt(thread, "Look at the matter.");
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let code = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == first => Some(failed.code),
        _ => None,
    });
    assert_eq!(code, Some(ErrorCode::AgentAuthRequired));
    assert_eq!(
        wait("a split", splits.recv()).unwrap(),
        LoginSplit {
            agent: codex(),
            lawyers_file_gone: false,
        }
    );
    events.until("the turn put back", |events| {
        events
            .position(|event| matches!(event, DomainEvent::TurnQueued(q) if q.behind_turn() == Some(first)))
            .is_some()
    });
    host.settle();
    assert!(held(&host, matter, thread));
    assert!(launches.lock().unwrap().is_empty(), "nothing launched");

    // The host's reused flag is false now: after desktop's leave order, the restart launches
    // with Clauseo's own login and no split check refuses it.
    fake.set(LoginLink::Own);
    wait("restart", host.handle.restart_agents(codex())).unwrap();
    events.until("the held turn ends", |events| {
        events.count(|event| matches!(event, DomainEvent::TurnEnded(ended) if ended.stop == StopReason::EndTurn)) == 1
    });
    assert_eq!(launches.lock().unwrap().len(), 1);
    assert!(splits.try_recv().is_err());
    events.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn a_lawyers_file_gone_is_a_split_that_says_so() {
    let dir = codex_dir("login_check_gone", "codex-handshake");
    let hook = Fake::new(LoginLink::Gone);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let splits = host.handle.login_splits();
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    let (first, _) = host.prompt(thread, "Look at the matter.");
    events.until_turn_ended(first);
    let split = wait("a split", splits.recv()).unwrap();
    assert!(split.lawyers_file_gone);
    host.shutdown();
}

#[test]
fn a_check_that_times_out_is_no_sign_in_answer() {
    let dir = codex_dir("login_check_timeout", "codex-handshake");
    let (fake, release) = Fake::held(LoginLink::Split);
    let hook = Arc::clone(&fake);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let splits = host.handle.login_splits();
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();

    let (first, _) = host.prompt(thread, "Look at the matter.");
    host.wait_armed(Duration::from_secs(5));
    host.clock.advance(Duration::from_secs(5));
    assert_eq!(events.until_turn_ended(first).stop, StopReason::Failed);
    let code = events.events().find_map(|event| match event {
        DomainEvent::TurnFailed(failed) if failed.turn == first => Some(failed.code),
        _ => None,
    });
    // Refused, but not as a sign-in: no split, no hold for the sign-in, the flag unchanged.
    assert_eq!(code, Some(ErrorCode::AgentLaunchFailed));
    drop(release);
    host.settle();
    events.drain_ready();
    assert!(splits.try_recv().is_err());
    assert!(!held(&host, matter, thread));
    let put_back = events.count(|event| matches!(event, DomainEvent::TurnQueued(_)));
    assert_eq!(put_back, 0, "{}", events.dump());
    host.refused(send_prompt(thread, "/login"), ErrorCode::SigninLoginShared);
    host.shutdown();
}

#[test]
fn remove_login_links_never_waits_on_the_actor() {
    let dir = codex_dir("login_check_links", "codex-handshake");
    // The hook holds a launch's check on its own thread meanwhile.
    let (fake, release) = Fake::held(LoginLink::Own);
    let hook = Arc::clone(&fake);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    host.prompt(thread, "Look at the matter.");
    wait("the hook asked", async {
        while fake.asked().is_empty() {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
    wait(
        "remove_login_links",
        host.handle.remove_login_links(codex()),
    )
    .unwrap();
    // The actor answers a query while the check still waits.
    assert_eq!(host.threads(matter).len(), 1);
    assert_eq!(fake.asked().len(), 1);
    drop(release);
    host.shutdown();
}

/// A subscriber that keeps every event's text: its message and fields.
struct Keep(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for Keep {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Text(String);
        impl tracing::field::Visit for Text {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let _written = write!(self.0, " {}={value:?}", field.name());
            }
        }
        let mut text = Text(String::new());
        event.record(&mut text);
        self.0.lock().unwrap().push(text.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn the_login_check_logs_no_path() {
    let lines = Arc::new(Mutex::new(Vec::new()));
    // nextest runs each test in its own process, so this default is this test's alone.
    let _set = tracing::subscriber::set_global_default(Keep(Arc::clone(&lines)));
    let dir = codex_dir("login_check_logs", "codex-handshake");
    let (fake, release) = Fake::held(LoginLink::Split);
    let hook = Arc::clone(&fake);
    let host = dir.start_with(0, |config| config.with_login_check(hook));
    let (_, thread, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CODEX);
    let mut events = host.events(StreamId::Thread(thread));
    wait("reused", host.handle.set_reused_login(codex(), true)).unwrap();
    // One check gives no answer in time, the next splits.
    let (first, _) = host.prompt(thread, "Look at the matter.");
    host.wait_armed(Duration::from_secs(5));
    host.clock.advance(Duration::from_secs(5));
    events.until_turn_ended(first);
    drop(release);
    let (second, _) = host.prompt(thread, "Look again.");
    events.until_turn_ended(second);
    wait(
        "remove_login_links",
        host.handle.remove_login_links(codex()),
    )
    .unwrap();
    host.settle();
    host.shutdown();

    let lines = lines.lock().unwrap();
    let about: Vec<&String> = lines.iter().filter(|line| line.contains("login")).collect();
    assert!(about.len() >= 3, "{lines:#?}");
    let home = home(&dir);
    let home = home.to_string_lossy();
    let data = dir.data_dir().to_string_lossy().into_owned();
    for line in &about {
        assert!(
            !line.contains(home.as_ref()) && !line.contains(&data),
            "{line}"
        );
    }
}
