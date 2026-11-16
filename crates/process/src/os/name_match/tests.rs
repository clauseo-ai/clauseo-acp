//! Every row of [`super::kind`]: a path read decides; without one, the short info's name and
//! state; without either, whether the process still exists. Uncertain is unknown, never other.
//! And [`super::starts_named`] over a scripted [`super::Reader`]: a process that exited since the
//! listing is skipped at every read, while one that may run unread makes the answer unknown.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use super::{Kind, Read, Reader, Seen, Short, Start, kind, starts_named};

const WORD: &str = "Microsoft Word";

/// The short info of a running process named `comm`.
fn running(comm: &[u8]) -> Short<'_> {
    Short {
        comm,
        exited: false,
    }
}

#[test]
fn a_readable_path_decides_by_its_file_name() {
    let never = || panic!("gone is asked only when nothing could be read");
    assert_eq!(
        kind(WORD, Some(b"Microsoft Word"), None, never),
        Kind::Named
    );
    assert_eq!(kind(WORD, Some(b"launchd"), None, never), Kind::Other);
    assert_eq!(
        kind(
            WORD,
            Some(b"Microsoft Word Helper"),
            Some(running(b"x")),
            never
        ),
        Kind::Other
    );
}

#[test]
fn a_running_word_whose_path_cannot_be_read_makes_the_answer_unknown() {
    assert_eq!(
        kind(WORD, None, Some(running(b"Microsoft Word")), || false),
        Kind::Unknown
    );
}

#[test]
fn a_running_other_process_whose_path_cannot_be_read_is_other() {
    assert_eq!(
        kind(WORD, None, Some(running(b"mds_stores")), || false),
        Kind::Other
    );
}

#[test]
fn an_exited_process_is_other_whatever_its_name() {
    let zombie = Some(Short {
        comm: b"Microsoft Word",
        exited: true,
    });
    assert_eq!(kind(WORD, None, zombie, || false), Kind::Other);
    assert_eq!(kind(WORD, None, None, || true), Kind::Other);
}

#[test]
fn a_live_process_with_neither_read_makes_the_answer_unknown() {
    assert_eq!(kind(WORD, None, None, || false), Kind::Unknown);
}

/// The comm holds at most 16 bytes of the name: a longer name matches by its first 16.
#[test]
fn a_name_longer_than_comm_matches_by_its_first_sixteen_bytes() {
    let long = "Microsoft PowerPoint";
    assert_eq!(
        kind(long, None, Some(running(b"Microsoft PowerP")), || false),
        Kind::Unknown
    );
    assert_eq!(
        kind(long, None, Some(running(b"Microsoft Power")), || false),
        Kind::Other
    );
}

/// What each read answers for one scripted pid.
struct Proc {
    file: Read<Vec<u8>>,
    short: Read<Seen>,
    gone: bool,
    start: Read<Start>,
}

/// A process that runs as `file`, readable by path, started at `start` seconds.
fn readable(file: &[u8], start: u64) -> Proc {
    Proc {
        file: Read::Got(file.to_vec()),
        short: Read::Failed,
        gone: false,
        start: Read::Got(Start::Running(at(start))),
    }
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

/// libproc as a script, by pid; a pid read that the script lacks panics.
struct Script(HashMap<u32, Proc>);

impl Script {
    fn of(procs: impl IntoIterator<Item = (u32, Proc)>) -> Self {
        Self(procs.into_iter().collect())
    }

    fn pid(&self, pid: u32) -> &Proc {
        self.0
            .get(&pid)
            .unwrap_or_else(|| panic!("pid {pid} not scripted"))
    }
}

impl Reader for Script {
    fn file(&self, pid: u32) -> Read<Vec<u8>> {
        self.pid(pid).file.clone()
    }
    fn short(&self, pid: u32) -> Read<Seen> {
        self.pid(pid).short.clone()
    }
    fn gone(&self, pid: u32) -> bool {
        self.pid(pid).gone
    }
    fn start(&self, pid: u32) -> Read<Start> {
        self.pid(pid).start.clone()
    }
}

/// The flake of clauseo-ai/clauseo-acp-private#415: a process exits between the listing and its
/// reads and waits as a zombie, so libproc answers ESRCH while `kill(pid, 0)` still finds it.
/// It is skipped, wherever its reads stopped, and the others are still found.
#[test]
fn a_process_that_exits_after_the_listing_is_skipped_at_each_read() {
    let path_gone = Proc {
        file: Read::Gone,
        ..readable(b"nextest", 1)
    };
    let short_gone = Proc {
        file: Read::Failed,
        short: Read::Gone,
        ..readable(b"x", 1)
    };
    let word_gone = Proc {
        start: Read::Gone,
        ..readable(WORD.as_bytes(), 3)
    };
    let script = Script::of([
        (10, readable(WORD.as_bytes(), 7)),
        (11, path_gone),
        (12, short_gone),
        (13, word_gone),
        (14, readable(b"launchd", 1)),
        (15, readable(WORD.as_bytes(), 9)),
    ]);
    assert_eq!(
        starts_named(WORD, Some(vec![10, 11, 12, 13, 14, 15]), &script),
        Some(vec![at(7), at(9)])
    );
}

/// A matched process that has exited (a zombie read with the zombie flag) has no start to give.
#[test]
fn an_exited_match_is_left_out() {
    let script = Script::of([(
        20,
        Proc {
            start: Read::Got(Start::Exited),
            ..readable(WORD.as_bytes(), 1)
        },
    )]);
    assert_eq!(
        starts_named(WORD, Some(vec![20]), &script),
        Some(Vec::new())
    );
}

/// A read that fails another way (EPERM, a short answer) still fails closed: a process that may be
/// the one runs unread, so the answer is unknown.
#[test]
fn a_live_process_that_cannot_be_read_still_makes_the_answer_unknown() {
    let unread = Proc {
        file: Read::Failed,
        short: Read::Failed,
        ..readable(b"x", 1)
    };
    let script = Script::of([(30, readable(WORD.as_bytes(), 1)), (31, unread)]);
    assert_eq!(starts_named(WORD, Some(vec![30, 31]), &script), None);

    let start_unread = Proc {
        start: Read::Failed,
        ..readable(WORD.as_bytes(), 1)
    };
    let script = Script::of([(32, start_unread)]);
    assert_eq!(starts_named(WORD, Some(vec![32]), &script), None);
}

/// A match whose start read fails another way is skipped only when `kill` says it is gone.
#[test]
fn a_match_whose_start_fails_and_that_is_gone_is_skipped() {
    let script = Script::of([(
        40,
        Proc {
            start: Read::Failed,
            gone: true,
            ..readable(WORD.as_bytes(), 1)
        },
    )]);
    assert_eq!(
        starts_named(WORD, Some(vec![40]), &script),
        Some(Vec::new())
    );
}

#[test]
fn a_failed_listing_is_unknown_and_an_empty_one_lists_none() {
    let script = Script::of([]);
    assert_eq!(starts_named(WORD, None, &script), None);
    assert_eq!(
        starts_named(WORD, Some(Vec::new()), &script),
        Some(Vec::new())
    );
}
