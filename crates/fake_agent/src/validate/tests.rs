use super::*;
use serde_json::json;

/// `value` as the SDK type `T` writes it back.
fn written<T: DeserializeOwned + Serialize>(value: &Value) -> Value {
    serde_json::to_value(T::deserialize(value).unwrap()).unwrap()
}

/// The JSON pointers, under `base`, of the top-level keys `sent` has that `back` lacks.
fn left_out(base: &str, sent: &Value, back: &Value) -> Vec<String> {
    sent.as_object()
        .unwrap()
        .keys()
        .filter(|key| back.get(key.as_str()).is_none())
        .map(|key| child(base, key))
        .collect()
}

#[test]
fn defaults_match_the_sdk() {
    let kind = serde_json::to_value(acp::ToolKind::default()).unwrap();
    let status = serde_json::to_value(acp::ToolCallStatus::default()).unwrap();
    let call = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
        "kind": "other", "status": "pending"});
    let notify = json!({"sessionId": "s", "update": call});
    for (place, default) in SKIPPED_DEFAULTS {
        let sdk = match place.pointer.rsplit('/').next() {
            Some("kind") => kind.as_str(),
            Some("status") => status.as_str(),
            _ => None,
        };
        assert_eq!(Some(*default), sdk, "{}", place.pointer);
        assert_eq!(place.within, TOOL_CALL, "{}", place.pointer);
    }
    let mut proven = left_out("/update", &call, &written::<acp::SessionUpdate>(&call));
    proven.extend(left_out(
        "/params/update",
        &call,
        &written::<acp::SessionNotification>(&notify)["update"],
    ));
    let mut listed: Vec<String> = SKIPPED_DEFAULTS
        .iter()
        .map(|(place, _)| place.pointer.to_owned())
        .collect();
    proven.sort();
    listed.sort();
    assert_eq!(proven, listed);

    let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
        "kind": "other", "status": "pending", "content": [], "locations": null});
    assert_eq!(validate(&Sent::Update(&update)), Ok(()));
}

/// A key the SDK type does not have is dropped on the way back, so it fails, whatever its value
/// and however deep it sits.
#[test]
fn keys_the_sdk_does_not_know_fail() {
    let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
        "unknownKey": {}});
    assert_eq!(
        validate(&Sent::Update(&update)),
        Err(Problem::Invalid(
            "/update/unknownKey: {} does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let result = json!({"protocolVersion": 1,
        "agentCapabilities": {"sessionCapabilities": {"lists": {}}}});
    assert_eq!(
        validate(&Sent::Result {
            method: "initialize",
            result: &result
        }),
        Err(Problem::Invalid(
            "/result/agentCapabilities/sessionCapabilities/lists: {} does not survive the SDK \
             schema (dropped)"
                .to_owned()
        ))
    );
    let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Look",
        "extra": "x"});
    assert_eq!(
        validate(&Sent::Update(&update)),
        Err(Problem::Invalid(
            r#"/update/extra: "x" does not survive the SDK schema (dropped)"#.to_owned()
        ))
    );
}

/// Each place in `SKIPPED_WHEN_EMPTY` is one the SDK leaves out when empty, in a message of
/// the method it names; and every key the SDK leaves out of those messages is listed.
#[test]
fn skipped_when_empty_matches_the_sdk() {
    let call = json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "x",
        "content": [], "locations": []});
    let notify = json!({"sessionId": "s", "update": call});
    let terminal = json!({"type": "terminal", "id": "t", "name": "x", "args": [], "env": {}});
    let create = json!({"sessionId": "s", "command": "c", "args": [], "env": []});
    // Each sample's keys are empty, so each key the SDK leaves out is one that may be skipped.
    let mut omitted: Vec<(&str, String)> = [
        (
            SESSION_UPDATE,
            left_out("/update", &call, &written::<acp::SessionUpdate>(&call)),
        ),
        (
            SESSION_UPDATE,
            left_out(
                "/params/update",
                &call,
                &written::<acp::SessionNotification>(&notify)["update"],
            ),
        ),
        (
            INITIALIZE,
            left_out(
                "/result/authMethods/*",
                &terminal,
                &written::<acp::AuthMethod>(&terminal),
            ),
        ),
        (
            TERMINAL_CREATE,
            left_out(
                "/params",
                &create,
                &written::<acp::CreateTerminalRequest>(&create),
            ),
        ),
    ]
    .into_iter()
    .flat_map(|(method, keys)| keys.into_iter().map(move |key| (method, key)))
    .collect();
    let mut listed: Vec<(&str, String)> = SKIPPED_WHEN_EMPTY
        .iter()
        .map(|place| (place.method, place.pointer.to_owned()))
        .collect();
    omitted.sort();
    listed.sort();
    assert_eq!(omitted, listed);
}

#[test]
fn edit_file_kind_does_not_survive() {
    let update = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit",
        "kind": "edit_file"});
    assert_eq!(
        validate(&Sent::Update(&update)),
        Err(Problem::Invalid(
            r#"/update/kind: "edit_file" does not survive the SDK schema (became "other")"#
                .to_owned()
        ))
    );
    let good = json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit",
        "kind": "edit"});
    assert_eq!(validate(&Sent::Update(&good)), Ok(()));
}

/// A value the SDK keeps but changes must fail: the scalar comparison is what catches it.
/// On a `tool_call_update` the SDK keeps `kind`, so `"edit_file"` comes back as `"other"`.
#[test]
fn a_changed_scalar_fails() {
    let update = json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1",
        "kind": "edit_file"});
    assert_eq!(
        validate(&Sent::Update(&update)),
        Err(Problem::Invalid(
            r#"/update/kind: "edit_file" does not survive the SDK schema (became "other")"#
                .to_owned()
        ))
    );
}

/// A key the SDK leaves out at one path is refused at any other: `status`, `kind` and `args`
/// on an `agent_message_chunk`, and `content` and `env` on a `plan`.
#[test]
fn keys_skipped_at_one_path_fail_at_another() {
    let chunk = json!({"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "hi"}});
    assert_eq!(validate(&Sent::Update(&chunk)), Ok(()));
    let with = |key: &str, value: Value| {
        let mut update = chunk.clone();
        update[key] = value;
        update
    };
    assert_eq!(
        validate(&Sent::Update(&with("status", json!("pending")))),
        Err(Problem::Invalid(
            r#"/update/status: "pending" does not survive the SDK schema (dropped)"#.to_owned()
        ))
    );
    assert_eq!(
        validate(&Sent::Update(&with("kind", json!("other")))),
        Err(Problem::Invalid(
            r#"/update/kind: "other" does not survive the SDK schema (dropped)"#.to_owned()
        ))
    );
    assert_eq!(
        validate(&Sent::Update(&with("args", Value::Null))),
        Err(Problem::Invalid(
            "/update/args: null does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let plan = json!({"sessionUpdate": "plan", "entries": []});
    assert_eq!(validate(&Sent::Update(&plan)), Ok(()));
    let plan_content = json!({"sessionUpdate": "plan", "entries": [], "content": []});
    assert_eq!(
        validate(&Sent::Update(&plan_content)),
        Err(Problem::Invalid(
            "/update/content: [] does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let plan_env = json!({"sessionUpdate": "plan", "entries": [], "env": {}});
    assert_eq!(
        validate(&Sent::Update(&plan_env)),
        Err(Problem::Invalid(
            "/update/env: {} does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let plan_both = json!({"sessionUpdate": "plan", "entries": [], "content": [], "env": {}});
    assert!(validate(&Sent::Update(&plan_both)).is_err());
    // The same keys are left out on a tool call, so they pass there.
    let call = json!({"sessionUpdate": "tool_call", "toolCallId": "t", "title": "x",
        "kind": "other", "status": "pending", "content": [], "locations": []});
    assert_eq!(validate(&Sent::Update(&call)), Ok(()));
    // The notification carries the update one level down, so its path is one level deeper.
    let notify = json!({"sessionId": "s", "update": {"sessionUpdate": "agent_message_chunk",
        "content": {"type": "text", "text": "hi"}, "status": "pending"}});
    assert_eq!(
        validate(&Sent::Notify {
            method: "session/update",
            params: Some(&notify),
        }),
        Err(Problem::Invalid(
            r#"/params/update/status: "pending" does not survive the SDK schema (dropped)"#
                .to_owned()
        ))
    );
}

/// `args` and `env` pass only where the SDK leaves them out: an empty terminal auth method
/// (whose `env` is a map, so `{}`) and `terminal/create`. An agent auth method has neither.
#[test]
fn args_and_env_pass_only_where_the_sdk_leaves_them_out() {
    let init = |auth: Value| json!({"protocolVersion": 1, "authMethods": [auth]});
    let terminal = json!({"type": "terminal", "id": "t", "name": "x", "args": [], "env": {}});
    assert_eq!(
        validate(&Sent::Result {
            method: "initialize",
            result: &init(terminal),
        }),
        Ok(())
    );
    let agent = json!({"id": "a", "name": "x", "args": []});
    assert_eq!(
        validate(&Sent::Result {
            method: "initialize",
            result: &init(agent),
        }),
        Err(Problem::Invalid(
            "/result/authMethods/0/args: [] does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let agent = json!({"id": "a", "name": "x", "env": {}});
    assert_eq!(
        validate(&Sent::Result {
            method: "initialize",
            result: &init(agent),
        }),
        Err(Problem::Invalid(
            "/result/authMethods/0/env: {} does not survive the SDK schema (dropped)".to_owned()
        ))
    );
    let create = json!({"sessionId": "s", "command": "c", "args": [], "env": []});
    assert_eq!(
        validate(&Sent::Request {
            method: "terminal/create",
            params: Some(&create),
        }),
        Ok(())
    );
    let read = json!({"sessionId": "s", "path": "/x", "args": []});
    assert_eq!(
        validate(&Sent::Request {
            method: "fs/read_text_file",
            params: Some(&read),
        }),
        Err(Problem::Invalid(
            "/params/args: [] does not survive the SDK schema (dropped)".to_owned()
        ))
    );
}

/// Arrays of different length fail with the counts, not by checking only the common prefix.
#[test]
fn array_length_mismatch_is_named() {
    let plan = json!({"sessionUpdate": "plan", "entries": [
        {"content": "a", "priority": "high", "status": "pending"},
        {"bogus": 1}
    ]});
    assert_eq!(
        validate(&Sent::Update(&plan)),
        Err(Problem::Invalid(
            "/update/entries: 2 items do not survive the SDK schema (became 1)".to_owned()
        ))
    );
}

#[test]
fn unknown_variant_and_method_fail() {
    let update = json!({"sessionUpdate": "_clauseo_test_unknown"});
    assert!(matches!(
        validate(&Sent::Update(&update)),
        Err(Problem::Invalid(_))
    ));
    let result = json!({"outcome": "injected"});
    assert_eq!(
        validate(&Sent::Result {
            method: "_session/steering",
            result: &result
        }),
        Err(Problem::UnknownMethod)
    );
    let error = json!({"code": -32000, "message": "Authentication required"});
    assert_eq!(validate(&Sent::Error(&error)), Ok(()));
}

/// The method names of one `…MethodNames` struct, read from its serialised fields. A method
/// added by an SDK bump, or by an `unstable_*` feature, shows up here without an edit.
fn method_names<T: Serialize>(names: &T) -> Vec<String> {
    let Value::Object(fields) = serde_json::to_value(names).unwrap() else {
        panic!("method names serialise as an object");
    };
    fields
        .values()
        .map(|name| name.as_str().expect("a method name is a string").to_owned())
        .collect()
}

/// Every method name the SDK lists has a type here, so a new SDK method is noticed. The
/// agent's names are its requests, and the notifications it receives.
#[test]
fn every_stable_method_has_a_type() {
    /// Notifications the agent receives, which have no response type to check.
    const RECEIVED_NOTIFICATIONS: &[&str] = &[acp::AGENT_METHOD_NAMES.session_cancel];
    /// Unstable methods the SDK lists only when a feature is on. `fake_agent` turns none on, but
    /// Cargo unifies features in a workspace build, where `clauseo-acp`'s `unstable_session_fork`
    /// and `unstable_llm_providers` add these; a scenario still sends them only with `raw`.
    /// Revisit on any `agent-client-protocol` bump: a method that becomes stable must leave this
    /// list and get a response type, or this skip hides it.
    const UNSTABLE: &[&str] = &[
        "session/fork",
        "providers/list",
        "providers/set",
        "providers/disable",
    ];
    for method in method_names(&acp::AGENT_METHOD_NAMES) {
        if UNSTABLE.contains(&method.as_str()) {
            continue;
        }
        assert!(
            result_check(&method).is_some() || RECEIVED_NOTIFICATIONS.contains(&method.as_str()),
            "{method} has no response type or received-notification entry"
        );
    }
    for method in method_names(&acp::CLIENT_METHOD_NAMES) {
        assert!(
            request_check(&method).is_some() || notify_check(&method).is_some(),
            "{method} has no request or notification type"
        );
    }
    for method in method_names(&acp::PROTOCOL_LEVEL_METHOD_NAMES) {
        assert!(
            notify_check(&method).is_some(),
            "{method} has no notification type"
        );
    }
}

#[test]
fn pointers_match_a_star_only_for_an_array_index() {
    assert!(pointer_matches("/a/b", "/a/b"));
    assert!(pointer_matches(
        "/result/authMethods/*/args",
        "/result/authMethods/0/args"
    ));
    assert!(pointer_matches("/a/*", "/a/12"));
    assert!(!pointer_matches("/a/*", "/a/x"));
    assert!(!pointer_matches("/a/*", "/a/"));
    assert!(!pointer_matches("/a/b", "/a/1"));
    assert!(!pointer_matches("/a/*", "/a/0/b"));
    assert!(!pointer_matches("/a/*/c", "/a/0/d"));
}
