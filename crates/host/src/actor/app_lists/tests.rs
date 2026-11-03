//! The comparison that decides whether a row goes out: every shown field but `last_activity`.

use clauseo_protocol::{
    AgentId, FolderState, LastStop, MatterId, MatterKind, MatterStatus, MatterSummary, StopReason,
    ThreadId, ThreadStatus, ThreadSummary, Timestamp,
};

use super::{same_matter, same_thread};

fn at(n: i64) -> Timestamp {
    Timestamp::new(1_800_000_000_000 + n).unwrap()
}

fn id(n: u128) -> uuid::Uuid {
    uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n)
}

fn matter() -> MatterSummary {
    MatterSummary {
        matter: MatterId::new(id(1)).unwrap(),
        name: "m".to_owned(),
        kind: MatterKind::Folder,
        last_activity: Some(at(1)),
        client: None,
        next_date: None,
        pinned: false,
        last_opened: None,
        status: MatterStatus::Idle,
        folder_display: "Documents/m".to_owned(),
        folder_state: FolderState::Found,
    }
}

fn thread() -> ThreadSummary {
    ThreadSummary {
        thread: ThreadId::new(id(2)).unwrap(),
        matter: MatterId::new(id(1)).unwrap(),
        title: None,
        agent: AgentId::new("fake-agent").unwrap(),
        status: ThreadStatus::Idle,
        queue_held: false,
        seen: true,
        snoozed_until: None,
        done: false,
        last_stop: None,
        last_activity: Some(at(1)),
    }
}

#[test]
fn last_activity_alone_is_no_change() {
    let later = MatterSummary {
        last_activity: Some(at(9)),
        ..matter()
    };
    assert!(same_matter(&matter(), &later));
    let later = ThreadSummary {
        last_activity: Some(at(9)),
        ..thread()
    };
    assert!(same_thread(&thread(), &later));
}

#[test]
fn every_other_shown_field_is_a_change() {
    let matters = [
        MatterSummary {
            status: MatterStatus::Ready,
            ..matter()
        },
        MatterSummary {
            pinned: true,
            ..matter()
        },
        MatterSummary {
            last_opened: Some(at(2)),
            ..matter()
        },
        MatterSummary {
            name: "n".to_owned(),
            ..matter()
        },
        MatterSummary {
            folder_display: "Elsewhere/m".to_owned(),
            ..matter()
        },
        MatterSummary {
            folder_state: FolderState::Missing,
            ..matter()
        },
    ];
    for changed in &matters {
        assert!(!same_matter(&matter(), changed), "{changed:?}");
    }
    let threads = [
        ThreadSummary {
            seen: false,
            ..thread()
        },
        ThreadSummary {
            last_stop: Some(LastStop {
                reason: StopReason::EndTurn,
                at: at(2),
            }),
            ..thread()
        },
        ThreadSummary {
            status: ThreadStatus::Running,
            ..thread()
        },
        ThreadSummary {
            title: Some("t".into()),
            ..thread()
        },
    ];
    for changed in &threads {
        assert!(!same_thread(&thread(), changed), "{changed:?}");
    }
}
