//! The scripted sign-in: the variable's form, each script's states in order, a cancel, the
//! calls it leaves to S18, and the merged events ending with S18's.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::{
    AgentsEvent, Clock, Found, InstallState, Installed, Method, PasteCode, SigninCode, SigninError,
    SigninState, Source,
};
use clauseo_protocol::AgentId;
use futures::StreamExt;
use futures::channel::mpsc;

use super::{EMAIL, FakeSignin, Script, Steps, merged, scripts_from};

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
