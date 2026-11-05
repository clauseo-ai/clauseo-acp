//! The composition rule's table: running states and `Failed` pass through, then the policy
//! block, `Firm`, `ApiKey`, and S18's state with `method: Plan`; and `Compose` sends each
//! change once, in order.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_agents::{Clock, Found, LoginFrom, SigninCode, SigninState, Source};
use clauseo_protocol::{
    AgentId, ErrorCode, FoundLogin, PlanLabel, SetupEvent, SignIn, SignInMethod, SignInSource,
};

use super::{Compose, Facts, compose, timestamp};
use crate::setup::SetupEvents;

fn claude() -> AgentId {
    AgentId::new("claude-acp").unwrap()
}

fn codex() -> AgentId {
    AgentId::new("codex-acp").unwrap()
}

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

const T: u64 = 1_700_000_000;

/// The facts with each of firm, key and blocked set (since `T`) or not.
fn facts(firm: bool, key: bool, blocked: bool) -> Facts {
    let since = |set: bool| set.then(|| at(T));
    Facts {
        firm: since(firm),
        key: since(key),
        blocked: since(blocked),
    }
}

fn s18_states() -> Vec<SigninState> {
    vec![
        SigninState::SignedOut { found: None },
        SigninState::Queued,
        SigninState::Waiting {
            since: at(T),
            nudge: false,
        },
        SigninState::DeviceCode {
            url: "https://auth.openai.com/codex/device".to_owned(),
            code: "ABCD-1234".to_owned(),
            expires_at: at(T + 900),
        },
        SigninState::LoginFound {
            from: LoginFrom::ClaudeCode,
            email: Some("a@example.com".to_owned()),
        },
        SigninState::SignedIn {
            email: Some("a@example.com".to_owned()),
            plan: Some("max".to_owned()),
            source: Source::Own,
            at: at(T),
        },
        SigninState::Failed {
            code: SigninCode::LinkExpired,
            at: at(T),
        },
    ]
}

fn running_or_failed(state: &SigninState) -> bool {
    matches!(
        state,
        SigninState::Queued
            | SigninState::Waiting { .. }
            | SigninState::DeviceCode { .. }
            | SigninState::Failed { .. }
    )
}

fn by(method: SignInMethod) -> SignIn {
    SignIn::SignedIn {
        email: None,
        plan: None,
        method,
        source: SignInSource::Own,
        at: timestamp(at(T)),
    }
}

#[test]
fn the_composition_table() {
    let blocked = SignIn::Failed {
        code: ErrorCode::SigninPolicyUnreadable,
        at: timestamp(at(T)),
    };
    let mut states: Vec<Option<SigninState>> = s18_states().into_iter().map(Some).collect();
    states.push(None);
    for s18 in &states {
        for (firm, key, block) in [
            (false, false, false),
            (false, true, false),
            (true, false, false),
            (true, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let got = compose(&claude(), s18.as_ref(), facts(firm, key, block));
            let plan = s18.as_ref().and_then(|state| super::wire(&claude(), state));
            let want = match s18 {
                Some(state) if running_or_failed(state) => plan,
                _ if block => Some(blocked.clone()),
                _ if firm => Some(by(SignInMethod::Firm)),
                _ if key => Some(by(SignInMethod::ApiKey)),
                _ => plan,
            };
            assert_eq!(
                got, want,
                "s18 {s18:?}, firm {firm}, key {key}, blocked {block}"
            );
        }
    }
}

#[test]
fn running_states_and_failed_pass_through_a_key_and_an_endpoint() {
    for state in s18_states().iter().filter(|state| running_or_failed(state)) {
        let got = compose(&codex(), Some(state), facts(true, true, true));
        assert_eq!(got, super::wire(&codex(), state), "{state:?}");
        assert!(
            !matches!(got, Some(SignIn::SignedIn { .. })),
            "a running sign-in is never hidden: {state:?}"
        );
    }
}

#[test]
fn s18_states_map_to_the_wire_with_the_plan_in_words() {
    let signed_in = SigninState::SignedIn {
        email: Some("a@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Reused,
        at: at(T),
    };
    assert_eq!(
        compose(&claude(), Some(&signed_in), Facts::default()),
        Some(SignIn::SignedIn {
            email: Some("a@example.com".to_owned()),
            plan: Some(PlanLabel("Claude Max".to_owned())),
            method: SignInMethod::Plan,
            source: SignInSource::Reused,
            at: timestamp(at(T)),
        })
    );
    let codex_plus = SigninState::SignedIn {
        email: None,
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: at(T),
    };
    assert!(matches!(
        compose(&codex(), Some(&codex_plus), Facts::default()),
        Some(SignIn::SignedIn { plan: Some(PlanLabel(words)), .. }) if words == "ChatGPT Plus"
    ));
    let unknown_plan = SigninState::SignedIn {
        email: None,
        plan: Some("mystery".to_owned()),
        source: Source::Own,
        at: at(T),
    };
    assert!(matches!(
        compose(&claude(), Some(&unknown_plan), Facts::default()),
        Some(SignIn::SignedIn { plan: None, .. })
    ));
    assert_eq!(
        compose(
            &codex(),
            Some(&SigninState::SignedOut {
                found: Some(Found::CodexLinkSplit)
            }),
            Facts::default()
        ),
        Some(SignIn::SignedOut {
            found: Some(FoundLogin::CodexLinkSplit)
        })
    );
    assert_eq!(
        compose(
            &claude(),
            Some(&SigninState::Failed {
                code: SigninCode::PortBusy,
                at: at(T)
            }),
            Facts::default()
        ),
        Some(SignIn::Failed {
            code: ErrorCode::SigninPortBusy,
            at: timestamp(at(T))
        })
    );
    assert_eq!(compose(&claude(), None, Facts::default()), None);
}

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> SystemTime {
        at(T)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn sent(feed: &mut futures::channel::mpsc::UnboundedReceiver<Arc<str>>) -> Vec<SignIn> {
    std::iter::from_fn(|| feed.try_recv().ok())
        .map(|text| {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            match serde_json::from_value(value["setup"].clone()).unwrap() {
                SetupEvent::SignInChanged { state, .. } => state,
                other => panic!("not a sign-in: {other:?}"),
            }
        })
        .collect()
}

#[test]
fn compose_sends_each_change_once_in_order_and_a_key_over_a_running_sign_in_waits() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    let signed_out = SigninState::SignedOut { found: None };
    compose.signin(&claude(), signed_out.clone());
    compose.signin(&claude(), signed_out);
    compose.signin(&claude(), SigninState::Queued);
    compose.set_key(&claude(), true);
    assert!(
        events.sign_in_running(&claude()),
        "the key does not hide Queued"
    );
    compose.signin(
        &claude(),
        SigninState::SignedIn {
            email: None,
            plan: None,
            source: Source::Own,
            at: at(T),
        },
    );
    compose.set_key(&claude(), false);
    assert_eq!(
        sent(&mut feed),
        [
            SignIn::SignedOut { found: None },
            SignIn::Queued,
            by(SignInMethod::ApiKey),
            SignIn::SignedIn {
                email: None,
                plan: None,
                method: SignInMethod::Plan,
                source: SignInSource::Own,
                at: timestamp(at(T)),
            },
        ]
    );
}
