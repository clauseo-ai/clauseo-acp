//! Every row of [`super::kind`]: a path read decides; without one, the short info's name and
//! state; without either, whether the process still exists. Uncertain is unknown, never other.

use super::{Kind, Short, kind};

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
