//! The wire's text as the tests read it: a window's command, and the setup events a feed sent.

use std::sync::Arc;

use clauseo_protocol::{AgentId, SetupCommand, SetupEvent};
use serde_json::json;

/// `command` as a window's JSON decodes, `fields` added to its agent.
pub(super) fn command(name: &str, agent: &AgentId, fields: &serde_json::Value) -> SetupCommand {
    let mut body = json!({ "agent": agent.as_str() });
    body.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    serde_json::from_value(json!({ name: body })).unwrap()
}

/// The setup events sent on `feed` so far, decoded.
pub(super) fn setup_events(
    feed: &mut futures::channel::mpsc::UnboundedReceiver<Arc<str>>,
) -> Vec<SetupEvent> {
    std::iter::from_fn(|| feed.try_recv().ok())
        .map(|text| {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            serde_json::from_value(value["setup"].clone()).unwrap()
        })
        .collect()
}
