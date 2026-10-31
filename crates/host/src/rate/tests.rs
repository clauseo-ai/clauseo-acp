use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin, PendingKey};
use clauseo_protocol::{
    AgentSessionId, CompactString, NoticeLevel, PlanEntry, ToolCallPatch, ToolCallStatus, TurnId,
};

use super::{Admitted, RateCap, RatedUpdate};

fn v7(n: u8) -> uuid::Uuid {
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid()
}

fn session() -> AgentSessionId {
    AgentSessionId::new("session-1").unwrap()
}

fn turn_origin() -> Origin {
    Origin::Turn(TurnId::new(v7(1)).unwrap())
}

fn update(origin: Origin, update: AgentUpdate) -> RatedUpdate {
    RatedUpdate(AcpEvent::Update {
        session: session(),
        origin,
        update,
    })
}

fn tool_patch(acp_id: &str, patch: ToolCallPatch) -> RatedUpdate {
    let acp_id = CompactString::from(acp_id);
    update(turn_origin(), AgentUpdate::ToolCallUpdate { acp_id, patch })
}

fn titled(acp_id: &str, title: &str) -> RatedUpdate {
    let patch = ToolCallPatch {
        title: Some(CompactString::from(title)),
        ..ToolCallPatch::default()
    };
    tool_patch(acp_id, patch)
}

fn notice(text: &str) -> RatedUpdate {
    let level = NoticeLevel::Info;
    update(
        turn_origin(),
        AgentUpdate::Notice {
            level,
            text: text.to_owned(),
        },
    )
}

fn plan(n: usize) -> RatedUpdate {
    let entry: PlanEntry = serde_json::from_value(serde_json::json!({
        "content": "step",
        "priority": "high",
        "status": "pending"
    }))
    .unwrap();
    update(turn_origin(), AgentUpdate::Plan(vec![entry; n]))
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A cap of 2 with both slots used at `ms(0)`.
fn full_cap() -> RateCap<RatedUpdate> {
    let mut cap = RateCap::with_cap(2);
    assert_eq!(cap.admit(ms(0), notice("1")), Admitted::Pass(notice("1")));
    assert_eq!(cap.admit(ms(1), notice("2")), Admitted::Pass(notice("2")));
    cap
}

#[test]
fn over_cap_merges_tool_call_patches() {
    let mut cap = full_cap();
    // The first held item arms the window's end; later ones do not.
    assert_eq!(
        cap.admit(ms(2), titled("a", "one")),
        Admitted::Held {
            arm: Some(ms(1000))
        }
    );
    let status = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        ..Default::default()
    };
    assert_eq!(
        cap.admit(ms(3), tool_patch("b", status)),
        Admitted::Held { arm: None }
    );
    assert_eq!(
        cap.admit(ms(4), titled("a", "two")),
        Admitted::Held { arm: None }
    );
    let late = ToolCallPatch {
        status: Some(ToolCallStatus::Failed),
        raw_output: None,
        ..Default::default()
    };
    assert_eq!(
        cap.admit(ms(5), tool_patch("a", late)),
        Admitted::Held { arm: None }
    );
    assert!(cap.is_holding());

    let released = cap.window_end(ms(1000));
    let merged_a = ToolCallPatch {
        title: Some(CompactString::from("two")),
        status: Some(ToolCallStatus::Failed),
        ..Default::default()
    };
    let b = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        ..Default::default()
    };
    assert_eq!(
        released,
        vec![tool_patch("a", merged_a), tool_patch("b", b)]
    );

    // The window ended: the next item passes.
    assert_eq!(
        cap.admit(ms(1001), titled("a", "x")),
        Admitted::Pass(titled("a", "x"))
    );
}

#[test]
fn non_mergeable_event_flushes_held_first() {
    let mut cap = full_cap();
    let _ = cap.admit(ms(2), titled("a", "one"));
    let _ = cap.admit(ms(3), plan(1));
    let _ = cap.admit(ms(4), plan(2));
    // Replace keeps the last value, at the place of the first one held.
    let flushed = cap.admit(ms(5), notice("other"));
    let Admitted::Flush(items) = flushed else {
        panic!("expected a flush")
    };
    assert_eq!(items, vec![titled("a", "one"), plan(2), notice("other")]);
    assert!(!cap.is_holding());

    // A permission request is not an update: it flushes too.
    let _ = cap.admit(ms(6), titled("a", "two"));
    let ask = RatedUpdate(AcpEvent::PermissionRequested {
        session: session(),
        key: PendingKey(1),
        acp_tool_call_id: CompactString::from("a"),
        options: Vec::new(),
    });
    let items: Vec<RatedUpdate> = cap.admit(ms(7), ask.clone()).into_iter().collect();
    assert_eq!(items, vec![titled("a", "two"), ask]);
}

#[test]
fn late_updates_never_merge_into_a_turns() {
    let mut cap = full_cap();
    let _ = cap.admit(ms(2), titled("a", "turn"));
    let late_origin = Origin::Late(TurnId::new(v7(1)).unwrap());
    let patch = ToolCallPatch {
        title: Some(CompactString::from("late")),
        ..Default::default()
    };
    let late = update(
        late_origin,
        AgentUpdate::ToolCallUpdate {
            acp_id: "a".into(),
            patch,
        },
    );
    let _ = cap.admit(ms(3), late.clone());
    assert_eq!(cap.flush(), vec![titled("a", "turn"), late]);
}

#[test]
fn a_new_window_releases_what_is_held() {
    let mut cap = full_cap();
    let _ = cap.admit(ms(2), titled("a", "one"));
    // The timer was late: the next item opens a new window, after what was held.
    let items: Vec<RatedUpdate> = cap
        .admit(ms(1500), titled("a", "two"))
        .into_iter()
        .collect();
    assert_eq!(items, vec![titled("a", "one"), titled("a", "two")]);
    assert!(cap.window_end(ms(2500)).is_empty());
}
