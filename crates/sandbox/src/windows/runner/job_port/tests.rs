//! The job port's notice, read from a fake port.

use super::{ACTIVE_PROCESS_LIMIT, TOO_MANY_PROGRAMS, watch};

/// `JOB_OBJECT_MSG_NEW_PROCESS`, `JOB_OBJECT_MSG_EXIT_PROCESS` and
/// `JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO`: messages that say nothing to the lawyer.
const OTHERS: [u32; 3] = [6, 7, 4];

fn notices(messages: Vec<u32>) -> Vec<String> {
    let mut said = Vec::new();
    watch(messages, |line| said.push(line.to_owned()));
    said
}

#[test]
fn a_fork_bomb_at_the_cap_gives_one_notice_for_the_session() {
    let mut messages = OTHERS.to_vec();
    messages.extend([ACTIVE_PROCESS_LIMIT; 500]);
    messages.extend(OTHERS);
    messages.push(ACTIVE_PROCESS_LIMIT);
    assert_eq!(notices(messages), [TOO_MANY_PROGRAMS]);
}

#[test]
fn a_session_under_the_cap_gives_no_notice() {
    assert_eq!(notices(OTHERS.repeat(100)), Vec::<String>::new());
}

#[test]
fn the_notice_says_the_lawyer_s_words() {
    assert_eq!(
        TOO_MANY_PROGRAMS,
        "A task tried to start too many programs at once, so Clauseo stopped it. Ask the \
         assistant to try a smaller step."
    );
}
