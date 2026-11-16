//! `last_exit` and its answer: the query is a plain string, the answer leaves out an empty list
//! and an unknown count, and each kind has its JSON name, which the store keeps too.

use clauseo_protocol::{
    Count, HostQuery, InterruptedAction, InterruptedKind, LastExit, QueryResult, Timestamp,
};

use super::fixtures::{matter_id, round_trips, thread_id, v7_text};

#[test]
fn last_exit_is_a_plain_string() {
    assert_eq!(
        serde_json::to_string(&HostQuery::LastExit).unwrap(),
        r#""last_exit""#
    );
    round_trips(&HostQuery::LastExit);
}

#[test]
fn the_answer_has_its_json() {
    let clean = QueryResult::LastExit(LastExit {
        unclean: false,
        run_started: Timestamp::new(1_790_000_000_000).unwrap(),
        interrupted: Vec::new(),
    });
    assert_eq!(
        serde_json::to_string(&clean).unwrap(),
        r#"{"last_exit":{"unclean":false,"run_started":1790000000000}}"#
    );
    round_trips(&clean);
    let interrupted = QueryResult::LastExit(LastExit {
        unclean: true,
        run_started: Timestamp::new(1_790_000_000_000).unwrap(),
        interrupted: vec![
            InterruptedAction {
                kind: InterruptedKind::Undo,
                matter: matter_id(1),
                thread: Some(thread_id(2)),
                files: Some(Count::new(3).unwrap()),
            },
            InterruptedAction {
                kind: InterruptedKind::Restore,
                matter: matter_id(1),
                thread: None,
                files: None,
            },
        ],
    });
    let json = format!(
        r#"{{"last_exit":{{"unclean":true,"run_started":1790000000000,"interrupted":[{{"kind":"undo","matter":"{m}","thread":"{t}","files":3}},{{"kind":"restore","matter":"{m}"}}]}}}}"#,
        m = v7_text(1),
        t = v7_text(2)
    );
    assert_eq!(serde_json::to_string(&interrupted).unwrap(), json);
    round_trips(&interrupted);
}

#[test]
fn each_kind_names_itself_as_its_json() {
    for kind in InterruptedKind::ALL {
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            format!("\"{}\"", kind.name())
        );
        assert_eq!(InterruptedKind::from_name(kind.name()), Some(kind));
    }
    assert_eq!(InterruptedKind::from_name("merge"), None);
}
