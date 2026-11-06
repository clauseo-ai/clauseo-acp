//! The scripted sign-in: the variable's form, each script's states in order, a cancel, the
//! calls it leaves to S18, and the merged events ending with S18's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::{
    AgentsEvent, Clock, Found, InstallState, Installed, Method, PasteCode, SigninCode, SigninError,
    SigninState, Source,
};
use clauseo_protocol::AgentId;
use futures::StreamExt;
use futures::channel::mpsc;

use super::{EMAIL, FakeSignin, Script, Status, Steps, merged, scripts_from, status_from};

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

fn at() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        at()
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

/// S18 stand-in: records the calls it gets; only the sign-in calls answer.
#[derive(Default)]
struct Real {
    calls: Mutex<Vec<&'static str>>,
}

impl Real {
    fn call(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
}

impl crate::setup::backend::SetupAgents for Real {
    fn install(&self, _: &AgentId) {}
    fn retry(&self, _: &AgentId, _: bool) {}
    fn state(&self, _: &AgentId) -> Option<InstallState> {
        None
    }
    fn installed(&self, _: &AgentId) -> Option<Installed> {
        None
    }
    fn sign_in(&self, _: &AgentId, _: Method) -> Result<(), SigninError> {
        self.call("sign_in");
        Ok(())
    }
    fn cancel_sign_in(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("cancel_sign_in");
        Ok(())
    }
    fn paste_code(&self, _: &AgentId, _: PasteCode) -> Result<(), SigninError> {
        self.call("paste_code");
        Ok(())
    }
    fn sign_out(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("sign_out");
        Ok(())
    }
    fn check_status(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("check_status");
        Ok(())
    }
    fn find_login(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("find_login");
        Ok(())
    }
    fn use_found_login(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("use_found_login");
        Ok(())
    }
    fn use_different_account(&self, _: &AgentId) -> Result<(), SigninError> {
        self.call("use_different_account");
        Ok(())
    }
    fn signin_state(&self, _: &AgentId) -> Option<SigninState> {
        None
    }
    fn sign_in_going(&self, _: &AgentId) -> bool {
        false
    }
    fn stop_using_login(&self, _: &AgentId, _: Option<Found>) -> Result<(), SigninError> {
        self.call("stop_using_login");
        Ok(())
    }
    fn install_tries(&self, _: &AgentId) -> Option<u32> {
        None
    }
    fn shutdown(&self, _: Duration) -> bool {
        true
    }
    fn set_network(&self, _: clauseo_agents::Network) {}
    fn read_account(&self, _: &AgentId) -> clauseo_agents::AccountFuture {
        self.call("read_account");
        Box::pin(std::future::ready(clauseo_agents::AccountRead::Failed))
    }
}

fn fake(hold: Duration) -> (FakeSignin, Arc<Real>, mpsc::UnboundedReceiver<AgentsEvent>) {
    let real = Arc::new(Real::default());
    let scripts = BTreeMap::from([(claude(), Script::Ok), (codex(), Script::Fail)]);
    let steps = Steps {
        queued_for: Duration::ZERO,
        hold,
    };
    let (fake, events) = FakeSignin::new(
        Arc::clone(&real) as Arc<dyn crate::setup::backend::SetupAgents>,
        scripts,
        Arc::new(Fixed),
        steps,
    );
    (fake, real, events)
}

/// The next `n` sign-in states on `events`.
fn states(events: &mut mpsc::UnboundedReceiver<AgentsEvent>, n: usize) -> Vec<SigninState> {
    futures::executor::block_on(events.by_ref().take(n).collect::<Vec<_>>())
        .into_iter()
        .map(|event| match event {
            AgentsEvent::Signin { state, .. } => state,
            other => panic!("not a sign-in state: {other:?}"),
        })
        .collect()
}

fn waiting() -> SigninState {
    SigninState::Waiting {
        since: at(),
        nudge: false,
    }
}

#[test]
fn the_variable_names_agents_and_scripts_and_drops_the_rest() {
    let scripts = scripts_from(Some(
        " claude-acp=ok, codex-acp = fail ,bad,x=maybe,=ok,Bad Id=ok",
    ));
    assert_eq!(
        scripts,
        BTreeMap::from([(claude(), Script::Ok), (codex(), Script::Fail)])
    );
    assert!(scripts_from(None).is_empty());
    assert!(scripts_from(Some("")).is_empty());
}

#[test]
fn ok_plays_queued_waiting_then_signed_in() {
    let (fake, real, mut events) = fake(Duration::ZERO);
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.sign_in(&claude(), Method::Browser).unwrap();
    let signed_in = SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: None,
        source: Source::Own,
        at: at(),
    };
    assert_eq!(
        states(&mut events, 3),
        [SigninState::Queued, waiting(), signed_in.clone()]
    );
    assert_eq!(setup.signin_state(&claude()), Some(signed_in));
    assert!(!setup.sign_in_going(&claude()));
    assert!(real.calls.lock().unwrap().is_empty(), "S18 is not asked");
}

#[test]
fn fail_plays_queued_waiting_then_failed() {
    let (fake, _real, mut events) = fake(Duration::ZERO);
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.sign_in(&codex(), Method::Browser).unwrap();
    let failed = SigninState::Failed {
        code: SigninCode::Failed,
        at: at(),
    };
    assert_eq!(
        states(&mut events, 3),
        [SigninState::Queued, waiting(), failed]
    );
}

#[test]
fn a_cancel_while_waiting_ends_the_run_cancelled_and_its_end_never_comes() {
    let (fake, _real, mut events) = fake(Duration::from_secs(3600));
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.sign_in(&claude(), Method::Browser).unwrap();
    assert_eq!(states(&mut events, 2), [SigninState::Queued, waiting()]);
    assert!(setup.sign_in_going(&claude()));
    setup.cancel_sign_in(&claude()).unwrap();
    let cancelled = SigninState::Failed {
        code: SigninCode::Cancelled,
        at: at(),
    };
    assert_eq!(states(&mut events, 1), [cancelled]);
    assert_eq!(
        setup.cancel_sign_in(&claude()),
        Err(SigninError::NotRunning(claude()))
    );
}

#[test]
fn a_search_finds_nothing_and_a_sign_out_signs_out() {
    let (fake, _real, mut events) = fake(Duration::ZERO);
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.find_login(&claude()).unwrap();
    setup.sign_out(&claude()).unwrap();
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    assert_eq!(
        states(&mut events, 2),
        [none, SigninState::SignedOut { found: None }]
    );
    assert_eq!(
        setup.use_found_login(&claude()),
        Err(SigninError::NothingFound(claude()))
    );
}

#[test]
fn an_agent_with_no_script_goes_to_s18() {
    let (fake, real, _events) = fake(Duration::ZERO);
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    let other = AgentId::new("gemini").unwrap();
    setup.sign_in(&other, Method::Browser).unwrap();
    setup.find_login(&other).unwrap();
    setup.cancel_sign_in(&other).unwrap();
    assert_eq!(
        *real.calls.lock().unwrap(),
        ["sign_in", "find_login", "cancel_sign_in"]
    );
}

#[test]
fn the_merged_events_end_when_s18_s_end() {
    let (_keep, fake_events) = mpsc::unbounded::<AgentsEvent>();
    let s18 = futures::stream::iter([AgentsEvent::Signin {
        agent: claude(),
        state: SigninState::Queued,
    }]);
    let all: Vec<_> = futures::executor::block_on(merged(s18, fake_events).collect());
    assert_eq!(all.len(), 1, "ends though the scripts' sender lives");
}

/// The seam with Claude installed (`E2E_FAKE_INSTALLED`), its status answering `status`
/// counted from `limit`, and its sign-in scripted `ok`.
fn installed(
    status: Status,
    limit: Duration,
) -> (FakeSignin, Arc<Real>, mpsc::UnboundedReceiver<AgentsEvent>) {
    let (fake, real, events) = fake(Duration::ZERO);
    let fake = fake.with_status(BTreeSet::from([claude()]), status, limit);
    (fake, real, events)
}

const SIGNED_OUT: SigninState = SigninState::SignedOut { found: None };

#[test]
fn the_status_variable_names_signed_out_slow_or_hang() {
    assert_eq!(status_from(None), Status::SignedOut);
    assert_eq!(status_from(Some("")), Status::SignedOut);
    assert_eq!(status_from(Some("signed_out")), Status::SignedOut);
    assert_eq!(status_from(Some("other")), Status::SignedOut);
    assert_eq!(status_from(Some(" slow ")), Status::Slow);
    assert_eq!(status_from(Some("hang")), Status::Hang);
}

#[test]
fn an_installed_agent_s_status_says_signed_out_at_once_with_no_s18_call() {
    let (fake, real, mut events) = installed(Status::SignedOut, Duration::from_secs(3600));
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.check_status(&claude()).unwrap();
    assert_eq!(states(&mut events, 1), [SIGNED_OUT]);
    assert_eq!(setup.signin_state(&claude()), Some(SIGNED_OUT));
    assert!(real.calls.lock().unwrap().is_empty(), "S18 is not asked");
}

#[test]
fn slow_and_hang_say_checking_then_signed_out_after_their_wait() {
    for (status, limit_ms, at_least) in [(Status::Slow, 40, 60), (Status::Hang, 20, 140)] {
        let (fake, _real, mut events) = installed(status, Duration::from_millis(limit_ms));
        let setup: &dyn crate::setup::backend::SetupAgents = &fake;
        let began = clauseo_agents::testing::instant();
        setup.check_status(&claude()).unwrap();
        assert_eq!(
            states(&mut events, 1),
            [SigninState::Checking],
            "{status:?}"
        );
        assert_eq!(setup.signin_state(&claude()), Some(SigninState::Checking));
        assert_eq!(states(&mut events, 1), [SIGNED_OUT], "{status:?}");
        let took = clauseo_agents::testing::millis_since(began);
        assert!(took >= at_least, "{status:?} answered after {took} ms");
    }
}

#[test]
fn a_scripted_sign_in_pressed_while_checking_plays_and_the_check_s_end_never_comes() {
    let (fake, _real, mut events) = installed(Status::Hang, Duration::from_millis(20));
    let setup: &dyn crate::setup::backend::SetupAgents = &fake;
    setup.check_status(&claude()).unwrap();
    assert_eq!(states(&mut events, 1), [SigninState::Checking]);
    setup.sign_in(&claude(), Method::Browser).unwrap();
    let signed_in = SigninState::SignedIn {
        email: Some(EMAIL.to_owned()),
        plan: None,
        source: Source::Own,
        at: at(),
    };
    assert_eq!(
        states(&mut events, 3),
        [SigninState::Queued, waiting(), signed_in.clone()]
    );
    // past the check's 140 ms: its SignedOut was dropped, the sign-in stands
    futures::executor::block_on(async_io::Timer::after(Duration::from_millis(300)));
    assert_eq!(setup.signin_state(&claude()), Some(signed_in));
}
