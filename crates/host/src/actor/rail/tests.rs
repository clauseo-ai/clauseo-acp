//! The status rule as a table: one row per case, the threads' facts and the last opening in, the
//! matter's status out. And the folder as the rail shows it.

use std::path::{Path, PathBuf};

use clauseo_protocol::{MatterStatus, StopReason, ThreadId, ThreadStatus, Timestamp};

use super::{ThreadFacts, folder_display, matter_status};

fn at(n: i64) -> Timestamp {
    Timestamp::new(1_800_000_000_000 + n).unwrap()
}

fn thread(status: ThreadStatus) -> ThreadFacts {
    ThreadFacts {
        status,
        permission_pending: false,
        last_stop: None,
    }
}

fn ended(stop: StopReason, n: i64) -> ThreadFacts {
    ThreadFacts {
        last_stop: Some((stop, at(n))),
        ..thread(ThreadStatus::Idle)
    }
}

fn permission() -> ThreadFacts {
    ThreadFacts {
        permission_pending: true,
        ..thread(ThreadStatus::NeedsYou)
    }
}

fn paused(resets: Option<i64>) -> ThreadFacts {
    thread(ThreadStatus::PausedAtLimit {
        resets_at: resets.map(at),
    })
}

/// A row: its name, the threads' facts, the last opening, the status expected.
type Row = (&'static str, Vec<ThreadFacts>, Option<i64>, MatterStatus);

/// Rows of one thread that runs, waits or is new.
fn one_thread_rows() -> Vec<Row> {
    let queued = ThreadStatus::Queued {
        behind: ThreadId::new(uuid::Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0001,
        ))
        .unwrap(),
    };
    vec![
        ("no thread", vec![], None, MatterStatus::Idle),
        (
            "a new thread",
            vec![thread(ThreadStatus::Idle)],
            None,
            MatterStatus::Idle,
        ),
        (
            "archived says nothing",
            vec![ThreadFacts {
                status: ThreadStatus::Archived,
                ..ended(StopReason::EndTurn, 5)
            }],
            None,
            MatterStatus::Idle,
        ),
        (
            "starting",
            vec![thread(ThreadStatus::Starting)],
            None,
            MatterStatus::Working,
        ),
        (
            "running",
            vec![thread(ThreadStatus::Running)],
            None,
            MatterStatus::Working,
        ),
        (
            "queued behind another thread",
            vec![thread(queued)],
            None,
            MatterStatus::Working,
        ),
        (
            "a permission",
            vec![permission()],
            None,
            MatterStatus::NeedsPermission,
        ),
        (
            "a question",
            vec![thread(ThreadStatus::NeedsYou)],
            None,
            MatterStatus::NeedsAnswer,
        ),
        (
            "finished, never opened",
            vec![ended(StopReason::EndTurn, 5)],
            None,
            MatterStatus::Ready,
        ),
        (
            "finished after the opening",
            vec![ended(StopReason::EndTurn, 5)],
            Some(4),
            MatterStatus::Ready,
        ),
    ]
}

/// Rows of how the last turn ended, and the pause at the limit.
fn ending_rows() -> Vec<Row> {
    vec![
        (
            "finished before the opening",
            vec![ended(StopReason::EndTurn, 5)],
            Some(5),
            MatterStatus::Idle,
        ),
        (
            "cancelled",
            vec![ended(StopReason::Cancelled, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "cancelled before the opening",
            vec![ended(StopReason::Cancelled, 7)],
            Some(8),
            MatterStatus::Idle,
        ),
        (
            "failed",
            vec![ended(StopReason::Failed, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "cut off at max tokens",
            vec![ended(StopReason::MaxTokens, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "refused",
            vec![ended(StopReason::Refusal, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "an ending from a newer Clauseo",
            vec![ended(StopReason::Unknown, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "agent exited after finishing",
            vec![ThreadFacts {
                status: ThreadStatus::AgentExited,
                ..ended(StopReason::EndTurn, 5)
            }],
            None,
            MatterStatus::Ready,
        ),
        (
            "paused, reset known",
            vec![paused(Some(90))],
            None,
            MatterStatus::PausedAtLimit {
                resets_at: Some(at(90)),
            },
        ),
        (
            "paused, reset not given",
            vec![paused(None)],
            None,
            MatterStatus::PausedAtLimit { resets_at: None },
        ),
    ]
}

/// Rows where threads differ: the precedence.
fn precedence_rows() -> Vec<Row> {
    vec![
        (
            "two paused: the earliest reset",
            vec![paused(Some(90)), paused(None), paused(Some(60))],
            None,
            MatterStatus::PausedAtLimit {
                resets_at: Some(at(60)),
            },
        ),
        (
            "two stopped: the latest stop",
            vec![
                ended(StopReason::Cancelled, 7),
                ended(StopReason::Failed, 9),
            ],
            None,
            MatterStatus::Stopped { at: at(9) },
        ),
        (
            "a permission outranks everything",
            vec![
                thread(ThreadStatus::Running),
                paused(Some(1)),
                ended(StopReason::EndTurn, 5),
                thread(ThreadStatus::NeedsYou),
                permission(),
            ],
            None,
            MatterStatus::NeedsPermission,
        ),
        (
            "a question outranks a pause",
            vec![paused(Some(1)), thread(ThreadStatus::NeedsYou)],
            None,
            MatterStatus::NeedsAnswer,
        ),
        (
            "a pause outranks an answer ready",
            vec![ended(StopReason::EndTurn, 5), paused(None)],
            None,
            MatterStatus::PausedAtLimit { resets_at: None },
        ),
        (
            "an answer ready outranks work",
            vec![thread(ThreadStatus::Running), ended(StopReason::EndTurn, 5)],
            None,
            MatterStatus::Ready,
        ),
        (
            "work outranks a stop",
            vec![
                ended(StopReason::Cancelled, 7),
                thread(ThreadStatus::Running),
            ],
            None,
            MatterStatus::Working,
        ),
        (
            "a stop outranks idle",
            vec![thread(ThreadStatus::Idle), ended(StopReason::Cancelled, 7)],
            None,
            MatterStatus::Stopped { at: at(7) },
        ),
        (
            "a stop seen, an answer not",
            vec![
                ended(StopReason::Cancelled, 3),
                ended(StopReason::EndTurn, 9),
            ],
            Some(5),
            MatterStatus::Ready,
        ),
    ]
}

#[test]
fn the_status_rule_table() {
    let rows = [one_thread_rows(), ending_rows(), precedence_rows()].concat();
    for (name, threads, opened, expected) in rows {
        assert_eq!(matter_status(threads, opened.map(at)), expected, "{name}");
    }
}

#[test]
fn a_folder_shows_below_home_or_in_full() {
    let home = Path::new("/home/asha");
    let homes = [home.to_path_buf()];
    let folder = Path::new("/home/asha/Documents/Clauseo/State v. Kunal Desai");
    assert_eq!(
        folder_display(folder, &homes),
        // Built from its parts, so the expected text uses this OS's separator (`\` on Windows).
        ["Documents", "Clauseo", "State v. Kunal Desai"]
            .iter()
            .collect::<std::path::PathBuf>()
            .display()
            .to_string()
    );
    assert_eq!(
        folder_display(Path::new("/srv/cases/A"), &homes),
        "/srv/cases/A"
    );
    assert_eq!(folder_display(home, &homes), "/home/asha");
    // A sibling whose name starts with the home folder's is not below it.
    assert_eq!(
        folder_display(Path::new("/home/ashara/A"), &homes),
        "/home/ashara/A"
    );
    assert_eq!(folder_display(folder, &[]), folder.display().to_string());
    // The second spelling (the canonical one) counts too.
    let both = [PathBuf::from("/link/to/home"), home.to_path_buf()];
    assert_eq!(folder_display(Path::new("/home/asha/A"), &both), "A");
    // Case: ignored on macOS and Windows, as the file system does there; exact elsewhere.
    let shouted = Path::new("/HOME/Asha/A");
    let expected = if cfg!(any(target_os = "macos", windows)) {
        "A".to_owned()
    } else {
        shouted.display().to_string()
    };
    assert_eq!(folder_display(shouted, &homes), expected);
}
