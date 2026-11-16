//! A Claude launch under a custom endpoint with no key (decision record 0048, "A firm endpoint
//! needs a key"): Claude Code 2.1.293 would send the lawyer's own sign-in token to it as a
//! Bearer, so the host never starts it. Desktop's refusal, `agent.firm_key_needed`, is one the
//! host takes; and whatever desktop said, a Claude launch whose env has `ANTHROPIC_BASE_URL` and
//! nothing that turns that sign-in off fails with the same code before anything is spawned.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use clauseo_host::ProfileHook;
use clauseo_profile::PacksWritten;
use clauseo_protocol::{AgentId, ErrorCode, MatterId, StopReason, StreamId};
use serde_json::{Map, Value};

use crate::codex::{Recording, turn_failed};
use crate::support::{Dir, Running, Scenario, matter_with_thread, wait};

/// The agent id that makes a fake agent a Claude agent.
const CLAUDE: &str = "claude-acp";

/// The firm's endpoint, as desktop's hook sets it from a plan.
const GATEWAY: &str = "https://gateway.firm.example";

/// A profile hook whose launch env is whatever the test put in it last.
#[derive(Clone, Default)]
struct EnvHook(Arc<Mutex<BTreeMap<OsString, OsString>>>);

impl EnvHook {
    fn set(&self, vars: &[(&str, &str)]) {
        let mut env = self.0.lock().unwrap();
        env.clear();
        env.extend(
            vars.iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value))),
        );
    }
}

impl ProfileHook for EnvHook {
    fn session_meta(&self, _: &AgentId, _: Option<&PacksWritten>) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _: &AgentId) -> BTreeMap<OsString, OsString> {
        self.0.lock().unwrap().clone()
    }
}

/// One prompt on a new Claude thread in `matter`: how its turn ended, and its failure's code.
fn one_turn(host: &Running, matter: MatterId) -> (StopReason, Option<ErrorCode>) {
    let thread = host.create_thread(matter, CLAUDE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Summarise the brief.");
    let stop = events.until_turn_ended(turn).stop;
    (stop, turn_failed(&events, turn).map(|failed| failed.code))
}

#[test]
fn a_custom_endpoint_with_no_key_never_launches_claude_and_a_key_or_a_cloud_provider_does() {
    let dir = Dir::new(
        "firm_endpoint",
        &[(CLAUDE, Scenario::Bundled("claude-prompt-stream"))],
    );
    let recording = Recording::new(&dir);
    let launches = Arc::clone(&recording.envs);
    let launched = || launches.lock().unwrap().len();
    let hook = EnvHook::default();
    let for_host = hook.clone();
    let host = dir.start_launching(0, Arc::new(recording), move |config| {
        config.with_profile_hook(Box::new(for_host))
    });
    let (matter, _, _) = matter_with_thread(&dir, &host, "m", &[("brief.txt", "x\n")], CLAUDE);

    // Desktop's leak state: the plan's endpoint, the entry's empty key. Nothing is spawned.
    hook.set(&[("ANTHROPIC_BASE_URL", GATEWAY), ("ANTHROPIC_API_KEY", "")]);
    let before = launched();
    let (stop, code) = one_turn(&host, matter);
    assert_eq!(stop, StopReason::Failed);
    assert_eq!(code, Some(ErrorCode::AgentFirmKeyNeeded));
    assert_eq!(launched(), before, "nothing launched");

    // The firm's key stored: it launches.
    hook.set(&[
        ("ANTHROPIC_BASE_URL", GATEWAY),
        ("ANTHROPIC_API_KEY", "stored-key"),
    ]);
    assert_eq!(one_turn(&host, matter), (StopReason::EndTurn, None));

    // Bedrock with no key, as a plan's provider sets it: the cloud's credentials, it launches.
    hook.set(&[
        ("ANTHROPIC_BASE_URL", GATEWAY),
        ("ANTHROPIC_API_KEY", ""),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
    ]);
    assert_eq!(one_turn(&host, matter), (StopReason::EndTurn, None));

    // No plan: it launches as ever.
    hook.set(&[("ANTHROPIC_API_KEY", "")]);
    assert_eq!(one_turn(&host, matter), (StopReason::EndTurn, None));

    // Desktop's refusal is taken and refuses before the env is looked at; cleared, it launches.
    let claude = AgentId::new(CLAUDE).unwrap();
    let refused = Some(ErrorCode::AgentFirmKeyNeeded);
    wait(
        "set_launch_refused",
        host.handle.set_launch_refused(claude.clone(), refused),
    )
    .unwrap();
    let before = launched();
    assert_eq!(
        one_turn(&host, matter),
        (StopReason::Failed, Some(ErrorCode::AgentFirmKeyNeeded))
    );
    assert_eq!(launched(), before, "nothing launched");
    wait(
        "set_launch_refused",
        host.handle.set_launch_refused(claude, None),
    )
    .unwrap();
    assert_eq!(one_turn(&host, matter), (StopReason::EndTurn, None));
    host.shutdown();
}
