//! The lawyer's own change on a running session that the agent answers with an error
//! (`answers.rs`): `agent.setting_refused`, never `agent.protocol_error`, and its detail names
//! the method and code only, never the agent's text.

use clauseo_protocol::{AgentChoiceId, ErrorCode, HostCommand};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::rig::Rig;

/// One answered prompt, then a mode change the agent refuses with its own text.
const STEPS: &str = r#"{"expect":{"method":"session/prompt","as":"p1","params":{"sessionId":"sess-1"}}}
{"respond":{"to":"p1","result":{"stopReason":"end_turn"}}}
{"expect":{"method":"session/set_mode","as":"m1","params":{"sessionId":"sess-1","modeId":"plan"}}}
{"respond":{"to":"m1","error":{"code":-32603,"message":"SECRET agent text"}}}
"#;

#[test]
fn a_mode_change_the_agent_refuses_is_setting_refused() {
    let mut rig = Rig::start("setting-refused", STEPS);
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    let command = HostCommand::SetMode {
        thread,
        mode_id: AgentChoiceId::new("plan").unwrap(),
    };
    let refused = rig.dispatch(11, command).unwrap_err();
    assert_eq!(refused.code(), ErrorCode::AgentSettingRefused);
    let wire = refused.to_wire();
    assert!(wire.retryable(), "{wire:?}");
    assert!(!wire.message().contains("SECRET"), "{wire:?}");
    assert!(wire.message().contains("session/set_mode"), "{wire:?}");
    rig.finish();
}
