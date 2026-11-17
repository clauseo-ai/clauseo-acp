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
use crate::setup::inherited::Inherited;

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
        managed: None,
        ..Facts::default()
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
        refused: None,
    }
}

#[test]
fn the_composition_table() {
    let blocked = SignIn::Failed {
        code: ErrorCode::SigninPolicyUnreadable,
        at: timestamp(at(T)),
        host: None,
        setting: None,
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
            refused: None,
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
            at: timestamp(at(T)),
            host: None,
            setting: None,
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
                refused: None,
            },
        ]
    );
}

#[test]
fn each_find_login_ends_in_one_sign_in_changed_even_when_unchanged() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    let none = SigninState::SignedOut {
        found: Some(Found::NoLogin),
    };
    compose.signin(&claude(), none.clone());
    // A search that finds nothing again: its answer goes out though nothing changed.
    compose.searching(&claude());
    compose.signin(&claude(), none.clone());
    // The same state with no search waiting goes out no more.
    compose.signin(&claude(), none.clone());
    // A search the call refused waits for no answer.
    compose.searching(&claude());
    compose.no_search(&claude());
    compose.signin(&claude(), none);
    let off = SignIn::SignedOut {
        found: Some(FoundLogin::NoneFound),
    };
    assert_eq!(sent(&mut feed), [off.clone(), off]);
}

/// A "Use it" whose login Claude could not use here (S18's run-time fallback) says so on the
/// wire, apart from a plain `reuse_off`, so the card can tell the lawyer why.
#[test]
fn a_failed_use_it_goes_out_as_reuse_unusable() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    compose.signin(
        &claude(),
        SigninState::SignedOut {
            found: Some(Found::ReuseUnusable),
        },
    );
    let unusable = SignIn::SignedOut {
        found: Some(FoundLogin::ReuseUnusable),
    };
    assert_eq!(sent(&mut feed), [unusable]);
}

#[test]
fn a_search_waits_through_running_states_for_its_resting_answer() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    let off = SigninState::SignedOut {
        found: Some(Found::ReuseOff),
    };
    compose.signin(&claude(), off.clone());
    compose.searching(&claude());
    compose.signin(&claude(), SigninState::Queued);
    compose.signin(&claude(), off);
    let reuse_off = SignIn::SignedOut {
        found: Some(FoundLogin::ReuseOff),
    };
    assert_eq!(
        sent(&mut feed),
        [reuse_off.clone(), SignIn::Queued, reuse_off]
    );
}

/// The Windows test's defect 2: a sign-in that ended `Failed` (cancelled) before a key or an
/// endpoint arrived no longer hides it; the key's or the endpoint's line goes out at once.
#[test]
fn a_key_or_an_endpoint_after_a_failed_sign_in_clears_the_failure() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    let cancelled = SigninState::Failed {
        code: SigninCode::Cancelled,
        at: at(T),
    };
    compose.signin(&codex(), cancelled.clone());
    compose.set_key(&codex(), true);
    compose.signin(&claude(), cancelled);
    compose.set_firm(&claude(), true);
    let failed = SignIn::Failed {
        code: ErrorCode::SigninCancelled,
        at: timestamp(at(T)),
        host: None,
        setting: None,
    };
    assert_eq!(
        sent(&mut feed),
        [
            failed.clone(),
            by(SignInMethod::ApiKey),
            failed,
            by(SignInMethod::Firm),
        ]
    );
    // Removing them shows S18's state underneath: signed out, not the old failure.
    compose.set_key(&codex(), false);
    assert_eq!(sent(&mut feed), [SignIn::SignedOut { found: None }]);
}

/// A key saved over one already in memory clears a failure too; a running sign-in still
/// passes through a key (the backend ends it).
#[test]
fn a_saved_key_clears_a_failure_but_a_running_sign_in_still_shows() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    compose.set_key(&claude(), true);
    compose.signin(
        &claude(),
        SigninState::Failed {
            code: SigninCode::LinkExpired,
            at: at(T),
        },
    );
    compose.key_saved(&claude());
    compose.signin(&claude(), SigninState::Queued);
    compose.key_saved(&claude());
    assert_eq!(
        sent(&mut feed),
        [
            by(SignInMethod::ApiKey),
            SignIn::Failed {
                code: ErrorCode::SigninLinkExpired,
                at: timestamp(at(T)),
                host: None,
                setting: None,
            },
            by(SignInMethod::ApiKey),
            SignIn::Queued,
        ]
    );
}

/// A refused key: the key's line carries `refused` until a key is saved or removed; an
/// endpoint's or a plan's line never does.
#[test]
fn a_refused_key_shows_on_its_line_until_a_key_is_saved_or_removed() {
    let events = Arc::new(SetupEvents::default());
    let mut feed = events.follow();
    let compose = Compose::new(Arc::clone(&events), Arc::new(Fixed));
    compose.signin(&claude(), SigninState::SignedOut { found: None });
    compose.key_refused(&claude());
    compose.set_key(&claude(), true);
    compose.key_refused(&claude());
    compose.key_saved(&claude());
    compose.key_refused(&claude());
    compose.set_key(&claude(), false);
    compose.set_firm(&codex(), true);
    compose.set_key(&codex(), true);
    compose.key_refused(&codex());
    compose.set_firm(&codex(), false);
    let refused = SignIn::SignedIn {
        email: None,
        plan: None,
        method: SignInMethod::ApiKey,
        source: SignInSource::Own,
        at: timestamp(at(T)),
        refused: Some(timestamp(at(T))),
    };
    assert_eq!(
        sent(&mut feed),
        [
            SignIn::SignedOut { found: None },
            by(SignInMethod::ApiKey),
            refused.clone(),
            by(SignInMethod::ApiKey),
            refused,
            SignIn::SignedOut { found: None },
            by(SignInMethod::Firm),
            by(SignInMethod::ApiKey),
        ]
    );
}

/// A plan's endpoint with the first-party provider and no key in memory: the card says the
/// firm's key is needed (decision record 0048, "A firm endpoint needs a key") in place of
/// `Firm`; with the key it is `Firm`; an unreadable key still says so first, and a running
/// sign-in still passes through.
#[test]
fn a_firm_endpoint_with_no_key_shows_firm_key_needed_until_a_key_is_in_memory() {
    let out = SigninState::SignedOut { found: None };
    let own = Facts {
        own_signin: Some(at(T)),
        ..facts(true, false, false)
    };
    assert_eq!(
        compose(&claude(), Some(&out), own.clone()),
        Some(SignIn::Failed {
            code: ErrorCode::AgentFirmKeyNeeded,
            at: timestamp(at(T)),
            host: None,
            setting: None,
        })
    );
    let keyed = Facts {
        key: Some(at(T)),
        ..own.clone()
    };
    assert!(matches!(
        compose(&claude(), Some(&out), keyed),
        Some(SignIn::SignedIn {
            method: SignInMethod::Firm,
            ..
        })
    ));
    let unreadable = Facts {
        unreadable: Some(at(T)),
        ..own.clone()
    };
    assert!(matches!(
        compose(&claude(), Some(&out), unreadable),
        Some(SignIn::Failed {
            code: ErrorCode::AgentKeyUnreadable,
            ..
        })
    ));
    assert_eq!(
        compose(&claude(), Some(&SigninState::Queued), own),
        Some(SignIn::Queued)
    );
}

/// A custom endpoint no plan set, no key in memory: the card names its host with
/// `agent.endpoint_key_needed`, never the firm's code; a key lifts it, and a plan's endpoint
/// with no key says the firm's code first.
#[test]
fn an_endpoint_no_plan_set_with_no_key_names_its_host_until_a_key_is_in_memory() {
    let out = SigninState::SignedOut { found: None };
    let inherited = Facts {
        inherited: Some((
            at(T),
            Inherited {
                host: Some("proxy.example".to_owned()),
                setting: None,
            },
        )),
        ..Facts::default()
    };
    assert_eq!(
        compose(&claude(), Some(&out), inherited.clone()),
        Some(SignIn::Failed {
            code: ErrorCode::AgentEndpointKeyNeeded,
            at: timestamp(at(T)),
            host: Some("proxy.example".to_owned()),
            setting: None,
        })
    );
    let unparsed = Facts {
        inherited: Some((
            at(T),
            Inherited {
                host: None,
                setting: Some("CLAUDE_CODE_API_BASE_URL"),
            },
        )),
        ..Facts::default()
    };
    assert_eq!(
        compose(&claude(), Some(&out), unparsed),
        Some(SignIn::Failed {
            code: ErrorCode::AgentEndpointKeyNeeded,
            at: timestamp(at(T)),
            host: None,
            setting: Some("CLAUDE_CODE_API_BASE_URL".to_owned()),
        })
    );
    let keyed = Facts {
        key: Some(at(T)),
        ..inherited.clone()
    };
    assert!(matches!(
        compose(&claude(), Some(&out), keyed),
        Some(SignIn::SignedIn {
            method: SignInMethod::ApiKey,
            ..
        })
    ));
    let planned = Facts {
        own_signin: Some(at(T)),
        ..inherited
    };
    assert!(matches!(
        compose(&claude(), Some(&out), planned),
        Some(SignIn::Failed {
            code: ErrorCode::AgentFirmKeyNeeded,
            host: None,
            ..
        })
    ));
}
