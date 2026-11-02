//! Steering, pure parts: what `initialize` says about it (the capability) and what the agent
//! answers to `_session/steering` (the outcome). The send is in `driver/steer.rs`.

use clauseo_protocol::SteeringCapability;
use serde_json::{Map, Value, json};

use crate::types::SteerResult;

/// The `_meta` object of an `initialize` result, or of its `agentCapabilities`.
type Meta = Map<String, Value>;

/// The steering capability `initialize` gives: `Some` only when the result's
/// `_meta.steering.supported` is true (`native`); `idle_fallback_to_host` is set when
/// `agentCapabilities._meta.claudeCode.promptQueueing` is true.
pub(crate) fn capability(
    meta: Option<&Meta>,
    capabilities_meta: Option<&Meta>,
) -> Option<SteeringCapability> {
    flag(meta, "steering", "supported").then_some(SteeringCapability {
        native: true,
        idle_fallback_to_host: flag(capabilities_meta, "claudeCode", "promptQueueing"),
    })
}

/// Whether the host may steer this agent while no prompt waits: native steering with the idle
/// fallback. Without the fallback, the host must send the text as a prompt instead.
pub(crate) fn supported(capability: Option<SteeringCapability>) -> bool {
    capability.is_some_and(|capability| capability.native && capability.idle_fallback_to_host)
}

/// `meta[outer][inner]` is `true`.
fn flag(meta: Option<&Meta>, outer: &str, inner: &str) -> bool {
    meta.and_then(|meta| meta.get(outer))
        .and_then(|value| value.get(inner))
        .and_then(Value::as_bool)
        == Some(true)
}

/// The `_meta` a steer carries: the agent ends an idle steer as `promptRequired`, not as a turn
/// of its own.
pub(crate) fn request_meta() -> Meta {
    let mut meta = Map::new();
    meta.insert(
        "steering".to_owned(),
        json!({"idleBehavior": "promptRequired"}),
    );
    meta
}

/// The result of a `_session/steering` answer: its `outcome`, or `Failed` for an outcome this
/// client does not know or an answer with none.
pub(crate) fn outcome(answer: &Value) -> SteerResult {
    match answer.get("outcome").and_then(Value::as_str) {
        Some("injected") => SteerResult::Injected,
        Some("promptRequired") => SteerResult::PromptRequired,
        Some("startedNewTurn") => SteerResult::StartedNewTurn,
        _ => SteerResult::Failed,
    }
}
